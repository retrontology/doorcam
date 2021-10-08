import cv2
import numpy as np
from threading import Thread

SCREEN_ROTATION=cv2.ROTATE_90_CLOCKWISE
SCREEN_RESOLUTION=(480, 800)
SCREEN_DTYPE = np.uint16
CAPTURE_RESOLUTION=(1920, 1080)
FRAMEBUFFER_DEVICE='/dev/fb0'
BACKLIGHT_DEVICE='/sys/class/backlight/rpi_backlight/bl_power'
CAMERA_INDEX=0

def init_capture(index=CAMERA_INDEX, resolution=CAPTURE_RESOLUTION):
    cap = cv2.VideoCapture(index)
    cap.set(cv2.CAP_PROP_FOURCC, cv2.VideoWriter_fourcc('M', 'J', 'P', 'G'))
    cap.set(cv2.CAP_PROP_CONVERT_RGB, 1)
    cap.set(cv2.CAP_PROP_FRAME_WIDTH, resolution[0])
    cap.set(cv2.CAP_PROP_FRAME_HEIGHT, resolution[1]) 
    return cap

def fb_blank(data = 0, dev=FRAMEBUFFER_DEVICE):
    blank = np.array([[data]], dtype=SCREEN_DTYPE)
    blank = np.repeat(blank, SCREEN_RESOLUTION[0], 1)
    blank = np.repeat(blank, SCREEN_RESOLUTION[1], 0)
    print(blank.shape)
    fb_write(blank.tobytes(), dev)

def fb_write(data, dev=FRAMEBUFFER_DEVICE):
    with open(dev, 'wb') as fb:
        fb.write(data)

def backlight_set(flag: bool, dev=BACKLIGHT_DEVICE):
    if flag:
        out = b'0'
    else:
        out = b'1'
    with open(dev, 'wb') as backlight:
        backlight.write(out)

def play(image):
        image = cv2.rotate(image, SCREEN_ROTATION)
        image = cv2.resize(image, SCREEN_RESOLUTION)
        image = cv2.cvtColor(image, cv2.COLOR_BGR2BGR565)
        fb_write(image.tobytes())

def main():
    vid = init_capture(CAMERA_INDEX, CAPTURE_RESOLUTION)
    while True:
        ret, src = vid.read()
        Thread(target=play, args=(src,)).start()
        

if __name__ == '__main__':
    main()