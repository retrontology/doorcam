import cv2
import numpy as np
from threading import Thread
from doorscreen import *
from doorcam import *
from doorstream import *
from time import sleep

def main():
    cam = Camera(rotation=cv2.ROTATE_90_CLOCKWISE, convert_rgb=False)
    screen = Screen()
    while True:
        print(cam.fps)
        sleep(1)
        

if __name__ == '__main__':
    main()