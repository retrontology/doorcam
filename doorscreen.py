import cv2
import numpy as np

DEFAULT_FRAMEBUFFER_DEVICE='/dev/fb0'
DEFAULT_BACKLIGHT_DEVICE='/sys/class/backlight/rpi_backlight/bl_power'
DEFAULT_COLOR_CONV=cv2.COLOR_BGR2BGR565
DEFAULT_RESOLUTION=(480,800)
DEFAULT_DTYPE = np.uint16

class Screen():

    def __init__(self, resolution=DEFAULT_RESOLUTION, rotation=None, fbdev=DEFAULT_FRAMEBUFFER_DEVICE, bldev=DEFAULT_BACKLIGHT_DEVICE, color_conv=DEFAULT_COLOR_CONV, dtype=DEFAULT_DTYPE):
        self.resolution = resolution
        self.rotation = rotation
        self.fbdev = fbdev
        self.bldev = bldev
        self.dtype = dtype
        self.color_conv = color_conv

    def fb_blank(self, data = 0):
        blank = np.array([[data]], dtype=self.dtype)
        blank = np.repeat(blank, self.resolution[0], 1)
        blank = np.repeat(blank, self.resolution[1], 0)
        self.fb_write(blank.tobytes(), self.fbdev)

    def fb_write(self, data):
        with open(self.fbdev, 'wb') as fb:
            fb.write(data)

    def fb_write_image(self, image):
        image = self.process_image(image)
        self.fb_write(image.tobytes())

    def bl_set(self, flag: bool):
        if flag:
            out = b'0'
        else:
            out = b'1'
        with open(self.bldev, 'wb') as backlight:
            backlight.write(out)
    
    def process_image(self, image):
        if self.rotation != None:
            image = cv2.rotate(image, self.rotation)
        image = cv2.resize(image, self.resolution)
        image = cv2.cvtColor(image, self.color_conv)
        return image

    def turn_off(self):
        self.fb_blank()
        self.bl_set(False)
    
    def turn_on(self):
        self.bl_set(True)