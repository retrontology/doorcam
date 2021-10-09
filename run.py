import cv2
import numpy as np
from threading import Thread
from doorscreen import *
from doorcam import *
from doorstream import *
import time
from http.server import HTTPServer
from functools import partial


def main():
    cam = Camera()
    screen = Screen(cam, rotation=cv2.ROTATE_90_CLOCKWISE)
    stream_handler = partial(MJPGStream, cam)
    server = HTTPServer(('192.168.1.24', 8080), stream_handler)
    http_thread = Thread(target=server.serve_forever)
    http_thread.start()
    screen.play_camera()
        

if __name__ == '__main__':
    main()