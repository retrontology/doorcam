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
    cam = Camera(index=0, max_fps=30)
    screen = Screen(cam)
    #analysis_thread = Thread(target=analysis_loop)
    #analysis_thread.start()
    stream_handler = partial(MJPGStream, cam)
    server = HTTPServer((IP, PORT), stream_handler)
    http_thread = Thread(target=server.serve_forever, args=(cam, screen))
    http_thread.start()
        

if __name__ == '__main__':
    main()