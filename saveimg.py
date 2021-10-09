import cv2
import os
import numpy as np
from doorcam import *

DIR = 'images'
COUNT = 300

def main():
    cam = Camera(index=0, max_fps=20)
    if not os.path.isdir(DIR):
        os.mkdir(DIR)
    time.sleep(5)
    for i in range(COUNT):
        name = os.path.join(DIR, f'image-{i}.jpg')
        cv2.imwrite(name, cam.get_current_frame())
        print(f'Written: {name}')

if __name__ == '__main__':
    main()