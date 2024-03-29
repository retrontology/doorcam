from doorcam import Camera
from threading import Thread
import time
import datetime
import os
import shutil
import cv2
from logging import getLogger
from PIL import Image, ImageDraw, ImageFont
import subprocess
import shutil

TIME_FORMAT = "%Y-%m-%d_%H-%M-%S-%f"
TIMESTAMP_FORMAT = "%H:%M:%S %m/%d/%Y"
TRIM_DELAY = 86400
TRIM_CHECK_INTERVAL = 300
CAPTURE_DECODE_FLAGS = cv2.IMREAD_COLOR + cv2.IMREAD_LOAD_GDAL

class Capture():

    logger = getLogger('doorcam.capture')

    def __init__(self, camera: Camera, preroll_time, postroll_time, capture_path, timestamp, rotation, video_encode, keep_images, trim_old, trim_limit):
        self.camera = camera
        self.preroll = preroll_time
        self.postroll = postroll_time
        self.path = os.path.abspath(capture_path)
        self.rotation = rotation
        self.timestamp = timestamp
        self.video_encode = video_encode
        self.keep_images = keep_images
        if not os.path.isdir(self.path):
            os.mkdir(self.path)
        self.activate = False
        self.trim_old = trim_old
        self.trim_limit = trim_limit
        self.queue = CaptureQueue(self.camera, self.preroll)
        self.post_process_queue = []
        self.post_process_thread = Thread(target=self.post_process_loop, daemon=True)
        self.post_process_thread.start()
        self.capture_thread = Thread(target=self.capture_loop, daemon=True)
        self.capture_thread.start()
        if self.trim_old:
            self.trim_thread = Thread(target=self.trim_loop, daemon=True)
            self.trim_thread.start()
        self.camera.add_callback(self.trigger_frame_update)

    def capture_loop(self):
        self.frame_update = False
        now = time.time()
        while True:
            while not self.activate:
                time.sleep(0.001)
            self.activate = False
            now = time.time()
            start = now
            dirname = datetime.datetime.fromtimestamp(now).strftime(TIME_FORMAT)
            dirname = os.path.join(self.path, dirname)
            self.logger.info(f'Capturing event and storing images at {dirname}')
            if not os.path.isdir(dirname):
                os.mkdir(dirname)
            imgdir = os.path.join(dirname, 'images')
            if not os.path.isdir(imgdir):
                os.mkdir(imgdir)
            preroll = self.queue.queue.copy()
            while now - start < self.postroll:
                while not self.frame_update:
                    time.sleep(0.001)
                self.frame_update = False
                now = time.time()
                filename = datetime.datetime.fromtimestamp(now).strftime(TIME_FORMAT)
                filename = os.path.join(imgdir, filename)
                filename = filename + '.jpg'
                with open(filename, 'wb') as out:
                    out.write(self.camera.current_jpg)
                if self.activate:
                    self.activate = False
                    start = now
            for timestamp, image in preroll:
                filename = datetime.datetime.fromtimestamp(timestamp).strftime(TIME_FORMAT)
                filename = os.path.join(imgdir, filename)
                filename = filename + '.jpg'
                with open(filename, 'wb') as out:
                    out.write(image)
            self.post_process_queue.append(dirname)

    def post_process_loop(self):
        while True:
            while len(self.post_process_queue) == 0:
                time.sleep(1)
            try:
                self.post_process(self.post_process_queue.pop(0))
            except Exception as e:
                self.logger.error(e)
    
    def trim_loop(self):
        timestamp = time.time()
        while True:
            self.trim_dir()
            timestamp += TRIM_DELAY
            while time.time() < timestamp:
                time.sleep(TRIM_CHECK_INTERVAL)

    def trim_dir(self):
        events = os.listdir(self.path)
        valid_events = []
        for event in events:
            event = os.path.join(self.path, event)
            if os.path.isdir(event):
                try:
                    timestamp = datetime.datetime.strptime(os.path.basename(event), TIME_FORMAT)
                    valid_events.append((event, timestamp))
                except Exception as e:
                    self.logger.debug(f'{event} could not be parsed as a timestamp, ignoring for trim')
        if len(valid_events) > 0:
            count = 0
            now = datetime.datetime.now()
            cutoff = now - datetime.timedelta(days=30)
            self.logger.debug(f'Checking for events before {cutoff.strftime(TIME_FORMAT)}')
            for event in valid_events:
                if event[1] < cutoff:
                    self.logger.debug(f'Trimming {event[0]} as it is older than the specified date of {cutoff.strftime(TIME_FORMAT)}')
                    try:
                        shutil.rmtree(event[0])
                        count += 1
                    except Exception as e:
                        self.logger.error(e)
        else:
            self.logger.debug('Did not detect any valid event directories while trimming')
    
    def post_process(self, path):
        self.logger.debug(f'Post-processing images located at: {path}')
        img_path = os.path.join(path, 'images')
        p_img_path = self.process_images(img_path)
        if self.video_encode:
            video_file = os.path.basename(path) + '.mp4'
            video_file = os.path.join(path, video_file)
            self.encode_video(video_file, p_img_path)
        if self.timestamp or self.rotation:
            try:
                shutil.rmtree(p_img_path)
            except Exception as e:
                self.logger.error(e)
        if not self.keep_images:
            try:
                shutil.rmtree(img_path)
            except Exception as e:
                self.logger.error(e)

    def process_images(self, path):

        if not (self.timestamp or self.rotation):
            return path
        
        images = []
        for filename in os.listdir(path):
            if filename[-4:].lower() == '.jpg':
                images.append(filename)
        images.sort()

        post_path = os.path.join(path, 'post')
        os.mkdir(post_path)

        for filename in images:

            full_filepath = os.path.join(path, filename)
            image = Image.open(full_filepath)

            if self.rotation:
                match self.rotation:
                    case cv2.ROTATE_90_CLOCKWISE:
                        rotation = 270
                    case cv2.ROTATE_180:
                        rotation = 180
                    case cv2.ROTATE_90_COUNTERCLOCKWISE:
                        rotation = 90
                    case _:
                        rotation = 0
                if rotation:
                    image = image.rotate(rotation, expand=1)

            if self.timestamp:
                timestamp = datetime.datetime.strptime(filename[:-4], TIME_FORMAT)
                draw = ImageDraw.Draw(image)
                font = ImageFont.truetype('DejaVuSansMono-Bold.ttf', 36)
                margin = 50
                draw.text((margin, margin), timestamp.strftime(TIMESTAMP_FORMAT), font=font)
            
            outfile = os.path.join(post_path, os.path.basename(filename))
            image.save(outfile)
        
        return post_path

    def encode_video(self, path, imgpath):
        command = [
            'ffmpeg',
            '-pattern_type', 'glob',
            '-i', os.path.join(imgpath, '*.jpg'),
            '-r', str(self.camera.max_fps),
            '-c:v', 'h264_v4l2m2m',
            '-pix_fmt', 'yuv420p',
            '-b:v', '4M',
            path
        ]
        result = subprocess.run(command, capture_output=True)
        event = os.path.basename(os.path.dirname(path))
        if result.returncode != 0:
            self.logger.error(f'Could not encode {event}!: {result.stderr}')
        else:
            self.logger.info(f'Video of {event} encoded and saved to {path}')

    def trigger_capture(self):
        self.activate = True
    
    def trigger_frame_update(self, img):
        self.frame_update = True

class CaptureQueue():

    logger = getLogger('doorcam.capture.queue')

    def __init__(self, camera: Camera, preroll_time):
        self.camera = camera
        self.preroll = preroll_time
        self.queue = list()
        self.camera.add_callback(self.push)

    def trim(self, now=time.time()):
        if len(self.queue) > 0:
            #self.sort()
            cutoff = now - self.preroll
            while self.queue[0][0] < cutoff:
                self.queue.pop(0)
    
    def sort(self):
        self.queue.sort(key = lambda x: x[0])

    def push(self, image):
        now = time.time()
        self.trim(now)
        self.queue.append((now, image))