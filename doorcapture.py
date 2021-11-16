from doorcam import Camera
from threading import Thread
from queue import SimpleQueue
import time
import datetime
import os
import cv2
from logging import getLogger

TIME_FORMAT = "%Y-%m-%d_%H-%M-%S-%f"
TIMESTAMP_FORMAT = "%H:%M:%S %m/%d/%Y"

class Capture():

    logger = getLogger('doorcam.capture')

    def __init__(self, camera: Camera, preroll_time, postroll_time, capture_path, timestamp, video_encode):
        self.camera = camera
        self.preroll = preroll_time
        self.postroll = postroll_time
        self.path = os.path.abspath(capture_path)
        self.timestamp = timestamp
        self.video_encode = video_encode
        if not os.path.isdir(self.path):
            os.mkdir(self.path)
        self.activate = False
        self.queue = CaptureQueue(self.camera, self.preroll)
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
            Thread(target=self.queue.dump(imgdir), daemon=True).start()
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
            Thread(target=self.post_process, args=(dirname,), daemon= True).start()

    def post_process(self, path):
        imgpath = os.path.join(path, 'images')
        if self.timestamp:
            for file in os.listdir(imgpath):
                if file[-4:].lower() == '.jpg':
                    try:
                        image = cv2.imread(os.path.join(imgpath, file), flags=cv2.IMREAD_COLOR)
                        timestamp = datetime.datetime.strptime(file[:-4], TIME_FORMAT)
                        image = cv2.putText(image, timestamp.strftime(TIMESTAMP_FORMAT), (50,50), cv2.FONT_HERSHEY_COMPLEX, 1, (255,255,255))
                        cv2.imwrite(os.path.join(imgpath, file), image)
                    except Exception as e:
                        self.logger.error(e)
        if self.video_encode:
            for file in os.listdir(imgpath):
                if file[-4:].lower() == '.jpg':
                    pass

    def trigger_capture(self):
        self.activate = True
    
    def trigger_frame_update(self, img):
        self.frame_update = True

class CaptureQueue():

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
    
    def dump(self, path):
        export = self.queue.copy()
        for timestamp, image in export:
            filename = datetime.datetime.fromtimestamp(timestamp).strftime(TIME_FORMAT)
            filename = os.path.join(path, filename)
            filename = filename + '.jpg'
            with open(filename, 'wb') as out:
                out.write(image)