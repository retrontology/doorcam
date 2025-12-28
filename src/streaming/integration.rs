use super::server::StreamServer;
use super::stats::StreamingStats;
use crate::config::StreamConfig;
use crate::error::Result;
use crate::events::{DoorcamEvent, EventBus};
use crate::ring_buffer::RingBuffer;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration, Instant};
use tracing::{error, info, warn};

/// Integration layer between streaming server and ring buffer
/// Handles frame synchronization, rate limiting, and quality adaptation
pub struct StreamingIntegration {
    ring_buffer: Arc<RingBuffer>,
    event_bus: Arc<EventBus>,
    config: StreamConfig,
    stats: StreamingStats,
    target_fps: u32,
}

impl StreamingIntegration {
    pub fn new(
        config: StreamConfig,
        ring_buffer: Arc<RingBuffer>,
        event_bus: Arc<EventBus>,
        target_fps: u32,
    ) -> Result<Self> {
        Ok(Self {
            ring_buffer,
            event_bus,
            config,
            stats: StreamingStats::default(),
            target_fps,
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting streaming integration");

        let server_handle = {
            let server = StreamServer::new(
                self.config.clone(),
                Arc::clone(&self.ring_buffer),
                Arc::clone(&self.event_bus),
                self.target_fps,
            );

            tokio::spawn(async move {
                if let Err(e) = server.start().await {
                    error!("Stream server error: {}", e);
                }
            })
        };

        let sync_handle = self.start_frame_sync_monitor().await?;
        let stats_handle = self.start_stats_reporter().await?;

        tokio::select! {
            result = server_handle => {
                if let Err(e) = result {
                    error!("Stream server task error: {}", e);
                }
            }
            result = sync_handle => {
                if let Err(e) = result {
                    error!("Frame sync monitor task error: {}", e);
                }
            }
            result = stats_handle => {
                if let Err(e) = result {
                    error!("Stats reporter task error: {}", e);
                }
            }
        }

        Ok(())
    }

    async fn start_frame_sync_monitor(&self) -> Result<JoinHandle<()>> {
        let ring_buffer = Arc::clone(&self.ring_buffer);
        let event_bus = Arc::clone(&self.event_bus);

        let handle = tokio::spawn(async move {
            let mut event_receiver = event_bus.subscribe();
            let mut last_frame_id = 0u64;
            let mut frame_check_interval = interval(Duration::from_millis(100));

            info!("Frame synchronization monitor started");

            loop {
                tokio::select! {
                    _ = frame_check_interval.tick() => {
                        if let Some(latest_frame) = ring_buffer.get_latest_frame().await {
                            if latest_frame.id > last_frame_id {
                                let frames_missed = latest_frame.id - last_frame_id - 1;
                                if frames_missed > 0 {
                                    info!(
                                        "Frame sync: {} frames between {} and {} (missed: {})",
                                        latest_frame.id - last_frame_id,
                                        last_frame_id,
                                        latest_frame.id,
                                        frames_missed
                                    );
                                }
                                last_frame_id = latest_frame.id;
                            }
                        }
                    }

                    event_result = event_receiver.recv() => {
                        match event_result {
                            Ok(DoorcamEvent::FrameReady { frame_id, .. }) => {
                                info!("Frame sync: Frame {} ready for streaming", frame_id);
                            }
                            Ok(DoorcamEvent::SystemError { component, error }) if component == "camera" => {
                                warn!("Camera error detected, may affect streaming: {}", error);
                            }
                            Ok(_) => {}
                            Err(e) => {
                                error!("Frame sync monitor event error: {}", e);
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    async fn start_stats_reporter(&self) -> Result<JoinHandle<()>> {
        let event_bus = Arc::clone(&self.event_bus);
        let ring_buffer = Arc::clone(&self.ring_buffer);

        let handle = tokio::spawn(async move {
            let mut stats_interval = interval(Duration::from_secs(30));
            let mut last_stats_time = Instant::now();

            info!("Streaming stats reporter started");

            loop {
                stats_interval.tick().await;

                let elapsed = last_stats_time.elapsed().as_secs_f64();
                last_stats_time = Instant::now();

                let latest_frame = ring_buffer.get_latest_frame().await;
                if let Some(frame) = latest_frame {
                    if let Err(e) = event_bus
                        .publish(DoorcamEvent::FrameReady {
                            frame_id: frame.id,
                            timestamp: frame.timestamp,
                        })
                        .await
                    {
                        error!("Failed to publish FrameReady event: {}", e);
                    }
                }

                let _ = elapsed;
            }
        });

        Ok(handle)
    }

    pub fn stats(&self) -> &StreamingStats {
        &self.stats
    }
}
