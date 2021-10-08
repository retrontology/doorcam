import cv2
import numpy as np
from threading import Thread
from doorscreen import *
from doorcam import *

def main():
    cam = Camera(rotation=cv2.ROTATE_90_CLOCKWISE)
    screen = Screen()
    while True:
        screen.fb_write_image(cam.read())
        

if __name__ == '__main__':
    main()