import cv2
import numpy as np
from threading import Thread
from doorscreen import *
from doorcam import *
from doorstream import *
from dooranalyzer import *
import time
from functools import partial

IP='192.168.1.24'
PORT=8080
K=np.array([[539.8606873339231, 0.0, 999.745990731636], [0.0, 540.4889507343736, 541.3382370501859], [0.0, 0.0, 1.0]])
NK=np.array([[197.38024030151098, 0.0, 953.7677809843199], [0.0, 197.60994174831563, 540.5796661140536], [0.0, 0.0, 1.0]])
D=np.array([[-0.06300247530706406], [0.028367414247228113], [-0.018682028009339952], [0.0037199220124150604]])

def main():
    cam = Camera(undistort_K=K, undistort_D=D, undistort_NK=NK)
    screen = Screen(cam, rotation=cv2.ROTATE_90_CLOCKWISE, undistort=True)
    screen.play_camera()
    analyzer = Analyzer(cam, screen)
    stream_handler = partial(MJPGHandler, cam)
    server = MJPGServer((IP, PORT), stream_handler)
    #http_thread = Thread(target=server.serve_forever, daemon=True)
    #http_thread.start()
    server.serve_forever()
    #while True:
    #    print(f'Cam: {cam.fps} | Screen: {screen.fps} | Analyzer: {analyzer.fps}')
    #    time.sleep(1)

if __name__ == '__main__':
    main()