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
    cam = Camera()
    screen = Screen(cam, rotation=cv2.ROTATE_90_CLOCKWISE)
    screen.play_camera()
    #analysis_thread = Thread(target=analysis_loop, args=(cam, screen), daemon=True)
    #analysis_thread.start()
    stream_handler = partial(MJPGStream, cam)
    server = HTTPServer((IP, PORT), stream_handler)
    #http_thread = Thread(target=server.serve_forever, daemon=True)
    #http_thread.start()
    server.serve_forever()
    

if __name__ == '__main__':
    main()