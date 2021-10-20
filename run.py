import cv2
import numpy as np
from threading import Thread
from doorscreen import *
from doorcam import *
from doorstream import *
import time
from http.server import HTTPServer
from functools import partial

ANALYSIS_PERIOD=1
IP='192.168.1.24'
PORT=8080
K=np.array([[135.85627595186807, 0.0, 250.50826117772505], [0.0, 136.96410060270327, 134.91000137514757], [0.0, 0.0, 1.0]])
D=np.array([[-0.034783891502412054], [-0.059526871172676084], [0.06857836924819212], [-0.02426263352503455]])
K_SCALE=1.8

def analysis_loop(cam: Camera, screen: Screen):
    hog = cv2.HOGDescriptor()
    hog.setSVMDetector(cv2.HOGDescriptor_getDefaultPeopleDetector())
    while True:
        frame = cam.get_current_frame()
        if frame == None:
            time.sleep(1)
            continue
        # Do some analysis
        # if person found screen.play_camera()

def main():
    cam = Camera()
    screen = Screen(cam, rotation=cv2.ROTATE_90_CLOCKWISE, undistort_K=K, undistort_D=D, undistort_K_scale=1.8)
    screen.play_camera()
    #analysis_thread = Thread(target=analysis_loop, args=(cam, screen), daemon=True)
    #analysis_thread.start()
    stream_handler = partial(MJPGStream, cam)
    server = HTTPServer((IP, PORT), stream_handler)
    #http_thread = Thread(target=server.serve_forever, daemon=True)
    #http_thread.start()
    server.serve_forever()
    #while True:
    #    print(f'Cam: {cam.fps} | Screen: {screen.fps}')
    #    time.sleep(1)

if __name__ == '__main__':
    main()