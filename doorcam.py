import cv2

SCREEN_RESOLUTION='480x800'
FRAMEBUFFER_DEVICE='/dev/fb0'
BACKLIGHT_DEVICE='/sys/class/backlight/rpi_backlight/bl_power'
CAMERA_INDEX=0

def fb_write(data, dev=FRAMEBUFFER_DEVICE):
    with open(dev, 'wb') as frame:
        frame.write(data)

def backlight_set(flag: bool, dev=BACKLIGHT_DEVICE):
    if flag:
        out = b'0'
    else:
        out = b'1'
    with open(dev, 'wb') as backlight:
        backlight.write(out)

def main():
    vid = cv2.VideoCapture(CAMERA_INDEX)

if __name__ == '__main__':
    main()