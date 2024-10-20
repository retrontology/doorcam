import cv2
import numpy as np
from threading import Thread
import time
import logging

class Camera():

    logger = logging.getLogger('doorcam.camera')

    def __init__(self, index:int, resolution:tuple, rotation, max_fps:int, fourcc, undistort_K:np.array, undistort_D:np.array, update_callbacks:set=None):
        self.logger.debug(f'Initializing camera at index {index}')
        self.index = index
        self.resolution = resolution
        self.rotation = rotation
        self.fourcc = fourcc
        self.frame_count = 0
        self.max_fps = max_fps
        self.interval = 1/self.max_fps
        self.fps = 0
        self.undistort_K = undistort_K
        self.undistort_D = undistort_D
        self.current_jpg = None
        self.update_callbacks = update_callbacks
        self.open()
        self.capture_thread = Thread(target=self.capture_loop, daemon=True)
        self.capture_thread.start()
        self.fps_thread = Thread(target=self.fps_loop, daemon=True)
        self.fps_thread.start()
        self.logger.debug(f'Camera at index {index} is intialized!')
    
    def add_callback(self, callback):
        if self.update_callbacks != None:
            self.update_callbacks.add(callback)
        else:
            self.update_callbacks = set((callback,))
    
    def remove_callback(self, callback):
        if self.update_callbacks != None and callback in self.update_callbacks:
            if len(self.update_callbacks) == 1:
                self.update_callbacks = None
            else:
                self.update_callbacks.remove(callback)

    def capture_loop(self):
        while True:
            try:
                ret, frame = self.cap.read()
                if ret:
                    self.current_jpg = frame
                    self.frame_count += 1
                    if self.update_callbacks != None:
                        for callback in self.update_callbacks:
                            Thread(target=callback, args=(frame, ), daemon=True).start()
            except Exception as e:
                self.logger.error(e)
                time.sleep(1)
    
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