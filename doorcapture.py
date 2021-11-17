from doorcam import Camera
from threading import Thread
import time
import datetime
import os
import cv2
from logging import getLogger

TIME_FORMAT = "%Y-%m-%d_%H-%M-%S-%f"
TIMESTAMP_FORMAT = "%H:%M:%S %m/%d/%Y"

class Capture():

    logger = getLogger('doorcam.capture')

    def __init__(self, camera: Camera, preroll_time, postroll_time, capture_path, timestamp, video_encode, keep_images):
        self.camera = camera
        self.preroll = preroll_time
        self.postroll = postroll_time
        self.path = os.path.abspath(capture_path)
        self.timestamp = timestamp
        self.video_encode = video_encode
        self.keep_images = keep_images
        if not os.path.isdir(self.path):
            os.mkdir(self.path)
        self.activate = False
        self.queue = CaptureQueue(self.camera, self.preroll)
        self.post_process_queue = []
        self.post_process_thread = Thread(target=self.post_process_loop, daemon=True)
        self.post_process_thread.start()
        self.capture_thread = Thread(target=self.capture_loop, daemon=True)
        self.capture_thread.start()
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

    def post_process(self, path):
        imgpath = os.path.join(path, 'images')
        images = []
        for filename in os.listdir(imgpath):
            if filename[-4:].lower() == '.jpg':
                images.append(filename)
        if len(images) > 0 and (self.timestamp or self.video_encode):
            images.sort()
            if self.video_encode:
                video_file = os.path.basename(path) + '.mp4'
                video_file = os.path.join(path, video_file)
                video_writer = cv2.VideoWriter(video_file, cv2.VideoWriter_fourcc(*'mp4v'), self.camera.max_fps, self.camera.resolution)
            for filename in images:
                fullpath = os.path.join(imgpath, filename)
                try:
                    image = cv2.imread(fullpath, flags=cv2.IMREAD_COLOR)
                    if self.timestamp:
                        timestamp = datetime.datetime.strptime(filename[:-4], TIME_FORMAT)
                        image = cv2.putText(image, timestamp.strftime(TIMESTAMP_FORMAT), (50,50), cv2.FONT_HERSHEY_COMPLEX, 1, (255,255,255))
                        if self.keep_images:
                            cv2.imwrite(fullpath, image)
                    if self.video_encode:
                        video_writer.write(image)
                    if not self.keep_images:
                        try:
                            os.remove(fullpath)
                        except Exception as e:
                            self.logger.error(e)
                except Exception as e:
                    self.logger.error(e)
            if self.video_encode:
                video_writer.release()
            if not self.keep_images:
                try:
                    os.rmdir(imgpath)
                except Exception as e:
                    self.logger.error(e)

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