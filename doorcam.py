import cv2
import numpy as np
from threading import Thread
import time

DEFAULT_INDEX=0
DEFAULT_FOURCC=cv2.VideoWriter_fourcc('M', 'J', 'P', 'G')
DEFAULT_RESOLUTION=(1920,1080)

class Camera():

    def __init__(self, index=DEFAULT_INDEX, resolution=DEFAULT_RESOLUTION, rotation=None, fourcc=DEFAULT_FOURCC, convert_rgb:bool = False):
        self.index = index
        self.resolution = resolution
        self.rotation = rotation
        self.fourcc = fourcc
        self.frame_count = 0
        if convert_rgb:
            self.convert_rbg = 0
        else:
            self.convert_rbg = 1
        self.current_frame = None
        self.open()
        self.capture_thread = Thread(target=self.capture_loop)
        self.capture_thread.start()
        self.fps_thread = Thread(target=self.fps_loop)
        self.fps_thread.start()

    def capture_loop(self):
        while True:
            self.current_frame = self.read()
            self.frame_count += 1
    
    def fps_loop(self):
        checkpoint = time.time()
        while True:
            now = time.time()
            if now - checkpoint >= 1:
                self.fps = self.frame_count
                self.frame_count = 0
                checkpoint = now
            else:
                time.sleep(0.01)

    def get_current_frame(self):
        if self.rotation != None:
            image = cv2.rotate(self.current_frame, self.rotation)
            if self.rotation == cv2.ROTATE_90_CLOCKWISE or self.rotation == cv2.ROTATE_90_COUNTERCLOCKWISE:
                image = cv2.resize(image, (self.resolution[1], self.resolution[0]))
            return image
        else:
            return self.current_frame
    
    def open(self):
        self.cap = cv2.VideoCapture(self.index)
        self.cap.set(cv2.CAP_PROP_FOURCC, self.fourcc)
        self.cap.set(cv2.CAP_PROP_CONVERT_RGB, self.convert_rbg)
        self.cap.set(cv2.CAP_PROP_FRAME_WIDTH, self.resolution[0])
        self.cap.set(cv2.CAP_PROP_FRAME_HEIGHT, self.resolution[1]) 
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