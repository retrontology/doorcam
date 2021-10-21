import cv2
import numpy as np
from threading import Thread
from doorscreen import *
from doorcam import *
from doorstream import *
from dooranalyzer import *
import time
from http.server import HTTPServer
from functools import partial

IP='192.168.1.24'
PORT=8080
K=np.array([[543.4251038074723, 0.0, 1002.0330447109002], [0.0, 547.8564024108131, 539.6400055005903], [0.0, 0.0, 1.0]])
D=np.array([[-0.034783891502412054], [-0.059526871172676084], [0.06857836924819212], [-0.02426263352503455]])
K_SCALE=1.8

def main():
    cam = Camera(undistort_K=K, undistort_D=D, undistort_K_scale=1.8)
    screen = Screen(cam, rotation=cv2.ROTATE_90_CLOCKWISE, undistort=False)
    screen.play_camera()
    analyzer = Analyzer(cam, screen)
    stream_handler = partial(MJPGStream, cam)
    server = HTTPServer((IP, PORT), stream_handler)
    #http_thread = Thread(target=server.serve_forever, daemon=True)
    #http_thread.start()
    server.serve_forever()
    #while True:
    #    print(f'Cam: {cam.fps} | Screen: {screen.fps} | Analyzer: {analyzer.fps}')
    #    time.sleep(1)

if __name__ == '__main__':
    main()