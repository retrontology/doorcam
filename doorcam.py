import cv2
import numpy as np
from threading import Thread
import time

class Camera():

    def __init__(self, index:int, resolution:tuple, rotation, max_fps:int, fourcc, undistort_K:np.array, undistort_D:np.array):
        self.index = index
        self.resolution = resolution
        self.rotation = rotation
        self.fourcc = fourcc
        self.frame_count = 0
        self.max_fps = max_fps
        self.fps = 0
        self.undistort_K = undistort_K
        self.undistort_D = undistort_D
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