import yaml
import os
import cv2
import numpy as np

DEFAULT_ANALYSIS_DELTA_THRESHOLD = 5
DEFAULT_ANALYSIS_CONTOUR_MIN_AREA = 5000
DEFAULT_ANALYSIS_MAX_FPS = 5
DEFAULT_CAMERA_INDEX=0
DEFAULT_CAMERA_FORMAT='MJPG'
DEFAULT_CAMERA_RESOLUTION='1920x1080'
#DEFAULT_CAMERA_FOURCC=cv2.VideoWriter_fourcc('M', 'J', 'P', 'G')
#DEFAULT_CAMERA_RESOLUTION=(1920,1080)
DEFAULT_CAMERA_MAX_FPS=30
DEFAULT_FRAMEBUFFER_DEVICE='/dev/fb0'
DEFAULT_FRAMEBUFFER_DTYPE='uint16'
DEFAULT_FRAMEBUFFER_RESOLUTION='480x800'
#DEFAULT_FRAMEBUFFER_DTYPE = np.uint16
#DEFAULT_FRAMEBUFFER_RESOLUTION=(480,800)
DEFAULT_BACKLIGHT_DEVICE='/sys/class/backlight/rpi_backlight/bl_power'
DEFAULT_TOUCH_DEVICE='/dev/input/event1'
DEFAULT_FRAMEBUFFER_COLOR_CONV='COLOR_BGR2BGR565'
#DEFAULT_FRAMEBUFFER_COLOR_CONV=cv2.COLOR_BGR2BGR565
DEFAULT_SCREEN_ACTIVATION_PERIOD = 10

class Config(dict):

    def __init__(self, path, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.path = path
        self.load_defaults()
        if os.path.isfile(path):
            self.load()
    
    def __del__(self, *args, **kwargs):
        self.save()
        super().__del__(*args, **kwargs)
    
    def load(self):
        with open(self.path, 'r') as stream:
            try:
                self.update(yaml.safe_load(stream).copy())
            except yaml.YAMLError as e:
                print(e)
    
    def load_defaults(self):
        analysis_configs = {
            'delta_threshold': DEFAULT_ANALYSIS_DELTA_THRESHOLD,
            'contour_minimum_area': DEFAULT_ANALYSIS_CONTOUR_MIN_AREA,
            'max_fps': DEFAULT_ANALYSIS_MAX_FPS
        }
        self.setdefault('analyzer', analysis_configs)
        camera_configs = {
            'index': DEFAULT_CAMERA_INDEX,
            'format': DEFAULT_CAMERA_FORMAT,
            'resolution': DEFAULT_CAMERA_RESOLUTION,
            'max_fps': DEFAULT_CAMERA_MAX_FPS
        }
        self.setdefault('camera', camera_configs)
        screen_configs = {
            'framebuffer_device': DEFAULT_FRAMEBUFFER_DEVICE,
            'dtype': DEFAULT_FRAMEBUFFER_DTYPE,
            'resolution': DEFAULT_FRAMEBUFFER_RESOLUTION,
            'backlight_device': DEFAULT_BACKLIGHT_DEVICE,
            'color_conversion': DEFAULT_FRAMEBUFFER_COLOR_CONV,
            'activation_period': DEFAULT_SCREEN_ACTIVATION_PERIOD
        }
        self.__setitem__('screen', screen_configs)
    
    def save(self):
        with open(self.path, 'w') as stream:
            try:
                stream.write(yaml.safe_dump(self.copy()))
            except yaml.YAMLError as e:
                print(e)
    
def rstring_to_rtuple(resolution:str):
    resolution = resolution.lower()
    resolution = resolution.split('x')
    if len(resolution) != 2:
        raise ImproperResolutionString
    else:
        return tuple(resolution)

def rtuple_to_rstring(resolution:tuple):
    if len(resolution) != 2 or any([type(x) != int for x in resolution]):
        raise ImproperResolutionTuple
    else:
        return 'x'.join(resolution)

class ImproperResolutionString(Exception):
    pass

class ImproperResolutionTuple(Exception):
    pass