import cv2
import numpy as np
from threading import Thread
from doorscreen import *
from doorcam import *
from doorstream import *

def main():
    cam = Camera(rotation=cv2.ROTATE_90_CLOCKWISE)
    screen = Screen()
    while True:
        try:
            screen.fb_write_image(cam.get_current_frame())
        except Exception as e:
            pass
        

if __name__ == '__main__':
    main()