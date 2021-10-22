import cv2
import os
import numpy as np
from doorcam import *

DIR = 'images'
COUNT = 10

def main():
    cam = Camera()
    if not os.path.isdir(DIR):
        os.mkdir(DIR)
    time.sleep(5)
    for i in range(COUNT):
        name = os.path.join(DIR, f'image-{i}.jpg')
        with open(name, 'wb') as out:
            out.write(cam.current_jpg)
        print(f'Written: {name}')
        time.sleep(0.5)

if __name__ == '__main__':
    main()