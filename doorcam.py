import cv2
import numpy as np
from threading import Thread
import time

DEFAULT_INDEX=0
DEFAULT_FOURCC=cv2.VideoWriter_fourcc('M', 'J', 'P', 'G')
DEFAULT_RESOLUTION=(1920,1080)
DEFAULT_MAX_FPS=30
DEFAULT_K_SCALE=1.8

class Camera():

    def __init__(self, index=DEFAULT_INDEX, resolution=DEFAULT_RESOLUTION, rotation=None, max_fps=DEFAULT_MAX_FPS, fourcc=DEFAULT_FOURCC, undistort_K=None, undistort_D=None, undistort_K_scale=DEFAULT_K_SCALE):
        self.index = index
        self.resolution = resolution
        self.rotation = rotation
        self.fourcc = fourcc
        self.frame_count = 0
        self.max_fps = max_fps
        self.fps = 0
        self.undistort_K = undistort_K
        self.undistort_D = undistort_D
        self.undistort_K_scale = undistort_K_scale
        self.current_jpg = None
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
                self.frame_count += 1
            ret, frame = self.cap.read()
            """ now = time.time()
            while(now - checkpoint < interval):
                time.sleep(0.001)
                now = time.time()
            checkpoint = now """
    
    def fps_loop(self):
        checkpoint = time.time()
        while True:
            now = time.time()
            if now - checkpoint >= 1:
                self.fps = self.frame_count
                self.frame_count = 0
                checkpoint = now
            else:
                time.sleep(0.1)
    
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