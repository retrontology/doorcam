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
K=np.array([[543.4251038074723, 0.0, 1002.0330447109002], [0.0, 547.8564024108131, 539.6400055005903], [0.0, 0.0, 1.0]])
D=np.array([[-0.034783891502412054], [-0.059526871172676084], [0.06857836924819212], [-0.02426263352503455]])
K_SCALE=1.8
DECODE_FLAGS = cv2.IMREAD_GRAYSCALE
DELTA_THRESHOLD = 5
CONTOUR_MIN_AREA = 5000

def analysis_loop(cam: Camera, screen: Screen):
    hog = cv2.HOGDescriptor()
    hog.setSVMDetector(cv2.HOGDescriptor_getDefaultPeopleDetector())
    frame_average = None
    while True:
        try:
            frame = cv2.imdecode(cam.current_jpg, DECODE_FLAGS)
        except Exception as e:
            print(e)
            time.sleep(1)
            continue
        frame = cv2.GaussianBlur(frame, (21,21), 0)
        if frame_average is None:
            frame_average = frame.copy().astype('float')
        cv2.accumulateWeighted(frame, frame_average, 0.5)
        frame_delta = cv2.absdiff(frame, cv2.convertScaleAbs(frame_average))
        ret, frame_threshold = cv2.threshold(frame_delta, DELTA_THRESHOLD, 255, cv2.THRESH_BINARY)
        frame_threshold = cv2.dilate(frame_threshold, None, iterations=2)
        contours, hierarchy = cv2.findContours(frame_threshold.copy(), cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
        activate = False
        for contour in contours:
            if cv2.contourArea(contour) > CONTOUR_MIN_AREA:
                activate = True
        if activate:
            screen.play_camera()

def main():
    cam = Camera(undistort_K=K, undistort_D=D, undistort_K_scale=1.8)
    screen = Screen(cam, rotation=cv2.ROTATE_90_CLOCKWISE, undistort=False)
    screen.play_camera()
    analysis_thread = Thread(target=analysis_loop, args=(cam, screen), daemon=True)
    analysis_thread.start()
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