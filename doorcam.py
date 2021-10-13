import cv2
import numpy as np
from threading import Thread
import time

DEFAULT_INDEX=0
DEFAULT_FOURCC=cv2.VideoWriter_fourcc('M', 'J', 'P', 'G')
DEFAULT_RESOLUTION=(1920,1080)
DEFAULT_MAX_FPS=30

class Camera():

    def __init__(self, index=DEFAULT_INDEX, resolution=DEFAULT_RESOLUTION, rotation=None, max_fps=DEFAULT_MAX_FPS, fourcc=DEFAULT_FOURCC):
        self.index = index
        self.resolution = resolution
        self.rotation = rotation
        self.fourcc = fourcc
        self.frame_count = 0
        self.max_fps = max_fps
        self.fps = 0
        self.current_frame = None
        self.open()
        self.capture_thread = Thread(target=self.capture_loop, daemon=True)
        self.capture_thread.start()
        self.fps_thread = Thread(target=self.fps_loop, daemon=True)
        self.fps_thread.start()

    def capture_loop(self):
        checkpoint = time.time()
        interval = 1.0/self.max_fps
        ret, frame = self.cap.read()
        while True:
            if ret:
                self.current_jpg = frame
                Thread(target=self.decode_frame, args=(frame,), daemon=True).start()
                self.frame_count += 1
            ret, frame = self.cap.read()
            now = time.time()
            while(now - checkpoint < interval):
                time.sleep(0.001)
                now = time.time()
            checkpoint = now
    
    def fps_loop(self):
        checkpoint = time.time()
        while True:
            now = time.time()
            if now - checkpoint >= 1:
                self.fps = self.frame_count
                self.frame_count = 0
                checkpoint = now
            else:
                time.sleep(0.001)
    
    def decode_frame(self, frame):
        self.current_frame = cv2.imdecode(frame, cv2.IMREAD_COLOR)

    def get_current_frame(self):
        if self.rotation != None:
            image = cv2.rotate(self.current_frame, self.rotation)
            if self.rotation == cv2.ROTATE_90_CLOCKWISE or self.rotation == cv2.ROTATE_90_COUNTERCLOCKWISE:
                image = cv2.resize(image, (self.resolution[1], self.resolution[0]))
            return image
        else:
            return self.current_frame
    
    def open(self):
        self.cap = cv2.VideoCapture(self.index, cv2.CAP_V4L2)
        self.cap.set(cv2.CAP_PROP_FOURCC, self.fourcc)
        self.cap.set(cv2.CAP_PROP_CONVERT_RGB, 0)
        self.cap.set(cv2.CAP_PROP_FRAME_WIDTH, self.resolution[0])
        self.cap.set(cv2.CAP_PROP_FRAME_HEIGHT, self.resolution[1])
        self.cap.set(cv2.CAP_PROP_FPS, self.max_fps)
        self.cap.set(cv2.CAP_PROP_BUFFERSIZE, 4)
        return self.cap
    
    def close(self):
        self.cap.release()
    
    def read(self):
        ret, image = self.cap.read()
        if ret:
            return image
        else:
            if self.cap.isOpened():
                raise CameraReadWhileClosed
            else:
                raise CameraReadError

class CameraReadError(Exception):
    pass

class CameraReadWhileClosed(CameraReadError):
    pass