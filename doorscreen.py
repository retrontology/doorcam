import cv2
import numpy as np
from doorcam import *
from evdev import InputDevice
import select

DEFAULT_FRAMEBUFFER_DEVICE='/dev/fb0'
DEFAULT_BACKLIGHT_DEVICE='/sys/class/backlight/rpi_backlight/bl_power'
DEFAULT_TOUCH_DEVICE='/dev/input/event0'
DEFAULT_COLOR_CONV=cv2.COLOR_BGR2BGR565
DEFAULT_RESOLUTION=(480,800)
DEFAULT_DTYPE = np.uint16
DEFAULT_PERIOD = 10
DECODE_FLAGS = cv2.IMREAD_REDUCED_COLOR_4

class Screen():

    def __init__(self, camera:Camera, resolution=DEFAULT_RESOLUTION, rotation=None, fbdev=DEFAULT_FRAMEBUFFER_DEVICE, bldev=DEFAULT_BACKLIGHT_DEVICE, color_conv=DEFAULT_COLOR_CONV, dtype=DEFAULT_DTYPE):
        self.camera = camera
        self.resolution = resolution
        self.rotation = rotation
        self.fbdev = fbdev
        self.bldev = bldev
        self.dtype = dtype
        self.color_conv = color_conv
        self.play_thread = None
        self.fps = 0
        self.frame_count = 0
        self.turn_off()

    def fb_blank(self, data = 0):
        blank = np.array([[data]], dtype=self.dtype)
        blank = np.repeat(blank, self.resolution[0], 1)
        blank = np.repeat(blank, self.resolution[1], 0)
        self.fb_write(blank.tobytes())

    def fb_write(self, data):
        with open(self.fbdev, 'wb') as fb:
            fb.write(data)

    def fb_write_image(self, image):
        try:
            image = self.process_image(image)
            self.fb_write(image.tobytes())
        except Exception as e:
            print(e)

    def bl_set(self, flag: bool):
        if flag:
            out = b'0'
        else:
            out = b'1'
        with open(self.bldev, 'wb') as backlight:
            backlight.write(out)
    
    def play_camera(self):
        if self.play_thread == None:
            self.activate = False
            self.play_thread = Thread(target=self.play_loop, daemon=True)
            self.play_thread.start()
            self.fps_thread = Thread(target=self.fps_loop, daemon=True)
            self.fps_thread.start()
        else:
            self.activate = True
    
    def fps_loop(self):
        checkpoint = time.time()
        while True:
            self.fps = self.frame_count
            self.frame_count = 0
            now = time.time()
            while now - checkpoint < 1:
                time.sleep(0.001)
                now = time.time()
            checkpoint = now

    
    def play_loop(self):
        self.turn_on()
        checkpoint = time.time()
        interval = 1.0/self.camera.max_fps
        while True:
            self.fb_write_image(self.camera.current_jpg)
            self.frame_count += 1
            now = time.time()
            while now - checkpoint < interval:
                time.sleep(0.001)
                now = time.time()
            checkpoint = now



    """ def play_loop(self):
        self.turn_on()
        start = time.time()
        now = time.time()
        interval = 1.0/self.camera.max_fps
        while now - start < DEFAULT_PERIOD:
            if self.activate:
                start = time.time()
                self.activate = False
            self.fb_write_image(self.camera.get_current_frame())
            now = time.time()
            int_start = now
            while now - int_start < interval:
                time.sleep(0.001)
                now = time.time()
            int_start = now
        self.turn_off()
        self.play_thread = None """

    def process_image(self, src):
        image = cv2.imdecode(src, DECODE_FLAGS)
        if self.rotation != None:
            image = cv2.rotate(image, self.rotation)
        image = cv2.resize(image, self.resolution)
        image = cv2.cvtColor(image, self.color_conv)
        return image

    def turn_off(self):
        self.fb_blank()
        self.bl_set(False)
    
    def turn_on(self):
        self.fb_blank()
        self.bl_set(True)