use crate::{
    error::{DoorcamError, Result},
    frame::FrameData,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufWriter};
use tracing::{debug, info};

/// Magic number for WAL file format: "DCAM"
const WAL_MAGIC: [u8; 4] = *b"DCAM";
const WAL_VERSION: u32 = 2;
const WAL_HEADER_SIZE: usize = 32;

/// Write-Ahead Log writer for persistent frame storage
pub struct WalWriter {
    file: BufWriter<File>,
    buffer: Vec<u8>,
    frame_count: u32,
    last_sync: Instant,
    event_id: String,
    wal_path: PathBuf,
    fps: u32,
}

impl WalWriter {
    /// Create a new WAL writer for an event
    pub async fn new(event_id: String, wal_dir: &Path, fps: u32) -> Result<Self> {
        // Ensure WAL directory exists
        tokio::fs::create_dir_all(wal_dir).await.map_err(|e| {
            DoorcamError::component("wal", &format!("Failed to create WAL directory: {}", e))
        })?;

        let wal_path = wal_dir.join(format!("{}.wal", event_id));

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&wal_path)
            .await
            .map_err(|e| {
                DoorcamError::component("wal", &format!("Failed to create WAL file: {}", e))
            })?;

        let mut writer = Self {
            file: BufWriter::new(file),
            buffer: Vec::with_capacity(2_000_000), // 2MB buffer
            frame_count: 0,
            last_sync: Instant::now(),
            event_id: event_id.clone(),
            wal_path: wal_path.clone(),
            fps,
        };

        // Write header
        writer.write_header().await?;

        info!(
            "Created WAL file for event {}: {}",
            event_id,
            wal_path.display()
        );
        Ok(writer)
    }

    /// Write WAL header
    async fn write_header(&mut self) -> Result<()> {
        let mut header = Vec::with_capacity(WAL_HEADER_SIZE);

        // Magic number
        header.extend_from_slice(&WAL_MAGIC);

        // Version
        header.extend_from_slice(&WAL_VERSION.to_le_bytes());

        // Event ID (first 16 bytes of UUID string as bytes)
        let event_id_bytes = self.event_id.as_bytes();
        let id_len = event_id_bytes.len().min(16);
        header.extend_from_slice(&event_id_bytes[..id_len]);
        header.resize(header.len() + (16 - id_len), 0); // Pad to 16 bytes

        // Frame count placeholder (will be updated on close)
        header.extend_from_slice(&0u32.to_le_bytes());

        // Camera FPS (0 if unknown)
        header.extend_from_slice(&self.fps.to_le_bytes());

        // Write header directly to file
        self.file.write_all(&header).await.map_err(|e| {
            DoorcamError::component("wal", &format!("Failed to write WAL header: {}", e))
        })?;

        Ok(())
    }

    /// Append a frame to the WAL
    pub async fn append_frame(&mut self, frame: &FrameData) -> Result<()> {
        // Write timestamp (as nanos since UNIX_EPOCH)
        let timestamp_nanos = frame
            .timestamp
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_nanos() as u64;

        self.buffer
            .extend_from_slice(&timestamp_nanos.to_le_bytes());

        // Write frame ID
        self.buffer.extend_from_slice(&frame.id.to_le_bytes());

        // Write data length
        let data_len = frame.data.len() as u32;
        self.buffer.extend_from_slice(&data_len.to_le_bytes());

        // Write JPEG data
        self.buffer.extend_from_slice(&frame.data);

        self.frame_count += 1;

        // Flush to OS buffer if buffer is large enough (~2MB = ~10 frames at 1080p)
        if self.buffer.len() >= 2_000_000 {
            self.flush_buffer().await?;
        }

        // Sync to disk every 1 second for crash safety
        if self.last_sync.elapsed() > Duration::from_secs(1) {
            self.sync().await?;
        }

        Ok(())
    }

    /// Flush buffer to OS
    async fn flush_buffer(&mut self) -> Result<()> {
        if !self.buffer.is_empty() {
            self.file.write_all(&self.buffer).await.map_err(|e| {
                DoorcamError::component("wal", &format!("Failed to write to WAL: {}", e))
            })?;

            debug!(
                "Flushed {} bytes to WAL for event {}",
                self.buffer.len(),
                self.event_id
            );
            self.buffer.clear();
        }
        Ok(())
    }

    /// Sync to disk
    async fn sync(&mut self) -> Result<()> {
        self.flush_buffer().await?;

        self.file
            .flush()
            .await
            .map_err(|e| DoorcamError::component("wal", &format!("Failed to flush WAL: {}", e)))?;

        self.file
            .get_ref()
            .sync_data()
            .await
            .map_err(|e| DoorcamError::component("wal", &format!("Failed to sync WAL: {}", e)))?;

        self.last_sync = Instant::now();
        debug!("Synced WAL to disk for event {}", self.event_id);

        Ok(())
    }

    /// Close the WAL and finalize
    pub async fn close(mut self) -> Result<PathBuf> {
        // Flush any remaining data
        self.flush_buffer().await?;
        self.file.flush().await.map_err(|e| {
            DoorcamError::component("wal", &format!("Failed to flush WAL on close: {}", e))
        })?;

        // Update frame count in header
        let mut file = self.file.into_inner();
        file.seek(std::io::SeekFrom::Start(24)).await.map_err(|e| {
            DoorcamError::component("wal", &format!("Failed to seek in WAL: {}", e))
        })?;

        file.write_all(&self.frame_count.to_le_bytes())
            .await
            .map_err(|e| {
                DoorcamError::component("wal", &format!("Failed to update frame count: {}", e))
            })?;

        file.sync_all().await.map_err(|e| {
            DoorcamError::component("wal", &format!("Failed to sync WAL on close: {}", e))
        })?;

        info!(
            "Closed WAL for event {} ({} frames)",
            self.event_id, self.frame_count
        );

        Ok(self.wal_path)
    }

    /// Get the number of frames written
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }
}

/// WAL reader for reading frames back
pub struct WalReader {
    path: PathBuf,
}

impl WalReader {
    /// Open a WAL file for reading
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Read all frames from the WAL
    pub async fn read_all_frames(&self) -> Result<Vec<FrameData>> {
        let mut reader = WalFrameReader::open(self.path.clone()).await?;
        let frame_count = reader.frame_count();

        info!(
            "Reading {} frames from WAL: {}",
            frame_count,
            self.path.display()
        );

        let mut frames = Vec::with_capacity(frame_count as usize);
        while let Some(frame) = reader.next_frame().await? {
            frames.push(frame);
        }

        info!("Read {} frames from WAL", frames.len());
        Ok(frames)
    }
}

/// Streaming WAL frame reader
pub struct WalFrameReader {
    file: File,
    frame_count: u32,
    path: PathBuf,
    fps: u32,
}

impl WalFrameReader {
    /// Open a WAL file and validate the header
    pub async fn open(path: PathBuf) -> Result<Self> {
        let mut file = File::open(&path).await.map_err(|e| {
            DoorcamError::component("wal", &format!("Failed to open WAL file: {}", e))
        })?;

        // Read and validate header
        let mut header = vec![0u8; WAL_HEADER_SIZE];
        file.read_exact(&mut header).await.map_err(|e| {
            DoorcamError::component("wal", &format!("Failed to read WAL header: {}", e))
        })?;

        // Validate magic number
        if header[0..4] != WAL_MAGIC {
            return Err(DoorcamError::component(
                "wal",
                "Invalid WAL file: bad magic number",
            ));
        }

        // Read version
        let version = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        if version != 1 && version != WAL_VERSION {
            return Err(DoorcamError::component(
                "wal",
                &format!("Unsupported WAL version: {}", version),
            ));
        }

        // Read frame count
        let frame_count = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
        let fps = if version >= 2 {
            u32::from_le_bytes([header[28], header[29], header[30], header[31]])
        } else {
            0
        };

        Ok(Self {
            file,
            frame_count,
            path,
            fps,
        })
    }

    /// Get the frame count recorded in the WAL header
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Get the FPS recorded in the WAL header (0 if unknown)
    pub fn fps(&self) -> u32 {
        self.fps
    }

    /// Read the next frame from the WAL (None on EOF)
    pub async fn next_frame(&mut self) -> Result<Option<FrameData>> {
        // Read timestamp (8 bytes)
        let mut timestamp_buf = [0u8; 8];
        match self.file.read_exact(&mut timestamp_buf).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => {
                return Err(DoorcamError::component(
                    "wal",
                    &format!("Failed to read timestamp: {}", e),
                ))
            }
        }
        let timestamp_nanos = u64::from_le_bytes(timestamp_buf);
        let timestamp = SystemTime::UNIX_EPOCH + Duration::from_nanos(timestamp_nanos);

        // Read frame ID (8 bytes)
        let mut id_buf = [0u8; 8];
        self.file.read_exact(&mut id_buf).await.map_err(|e| {
            DoorcamError::component("wal", &format!("Failed to read frame ID: {}", e))
        })?;
        let frame_id = u64::from_le_bytes(id_buf);

        // Read data length (4 bytes)
        let mut len_buf = [0u8; 4];
        self.file.read_exact(&mut len_buf).await.map_err(|e| {
            DoorcamError::component("wal", &format!("Failed to read data length: {}", e))
        })?;
        let data_len = u32::from_le_bytes(len_buf) as usize;

        // Read JPEG data
        let mut data = vec![0u8; data_len];
        self.file.read_exact(&mut data).await.map_err(|e| {
            DoorcamError::component("wal", &format!("Failed to read frame data: {}", e))
        })?;

        let frame = FrameData::new(
            frame_id,
            timestamp,
            data,
            0, // width unknown
            0, // height unknown
            crate::frame::FrameFormat::Mjpeg,
        );

        Ok(Some(frame))
    }

    /// Path for logging
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Find all WAL files in a directory
pub async fn find_wal_files(wal_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut wal_files = Vec::new();

    if !wal_dir.exists() {
        return Ok(wal_files);
    }

    let mut entries = tokio::fs::read_dir(wal_dir).await.map_err(|e| {
        DoorcamError::component("wal", &format!("Failed to read WAL directory: {}", e))
    })?;

    while let Some(entry) = entries.next_entry().await.map_err(|e| {
        DoorcamError::component("wal", &format!("Failed to read directory entry: {}", e))
    })? {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("wal") {
            wal_files.push(path);
        }
    }

    Ok(wal_files)
}

/// Delete a WAL file
pub async fn delete_wal(wal_path: &Path) -> Result<()> {
    tokio::fs::remove_file(wal_path).await.map_err(|e| {
        DoorcamError::component("wal", &format!("Failed to delete WAL file: {}", e))
    })?;

    debug!("Deleted WAL file: {}", wal_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_wal_write_read() {
        let temp_dir = TempDir::new().unwrap();
        let wal_dir = temp_dir.path();

        let event_id = "test-event-123".to_string();

        // Write frames
        let mut writer = WalWriter::new(event_id.clone(), wal_dir, 30)
            .await
            .unwrap();

        for i in 0..10 {
            let frame = FrameData::new(
                i,
                SystemTime::now(),
                vec![0u8; 1000],
                640,
                480,
                crate::frame::FrameFormat::Mjpeg,
            );
            writer.append_frame(&frame).await.unwrap();
        }

        let wal_path = writer.close().await.unwrap();

        // Read frames back
        let reader = WalReader::new(wal_path.clone());
        let frames = reader.read_all_frames().await.unwrap();

        assert_eq!(frames.len(), 10);

        // Cleanup
        delete_wal(&wal_path).await.unwrap();
    }
}
