import yaml
import os
import cv2
import numpy as np
from logging import Logger

DEFAULT_ANALYSIS_DELTA_THRESHOLD=5
DEFAULT_ANALYSIS_CONTOUR_MIN_AREA=5000
DEFAULT_ANALYSIS_MAX_FPS=5
DEFAULT_ANALYSIS_UNDISTORT=True
DEFAULT_ANALYSIS_UNDISTORT_BALANCE=1.0
DEFAULT_CAMERA_INDEX=0
DEFAULT_CAMERA_FORMAT='MJPG'
DEFAULT_CAMERA_RESOLUTION='1920x1080'
DEFAULT_CAMERA_ROTATION=None
DEFAULT_CAMERA_MAX_FPS=30
DEFAULT_CAMERA_K='[[539.8606873339231, 0.0, 999.745990731636], [0.0, 540.4889507343736, 541.3382370501859], [0.0, 0.0, 1.0]]'
DEFAULT_CAMERA_D='[[-0.06300247530706406], [0.028367414247228113], [-0.018682028009339952], [0.0037199220124150604]]'
DEFAULT_FRAMEBUFFER_DEVICE='/dev/fb0'
DEFAULT_FRAMEBUFFER_DTYPE='uint16'
DEFAULT_FRAMEBUFFER_RESOLUTION='480x800'
DEFAULT_FRAMEBUFFER_ROTATION='ROTATE_90_CLOCKWISE'
DEFAULT_FRAMEBUFFER_COLOR_CONV='COLOR_BGR2BGR565'
DEFAULT_FRAMEBUFFER_UNDISTORT=True
DEFAULT_FRAMEBUFFER_UNDISTORT_BALANCE=1.0
DEFAULT_BACKLIGHT_DEVICE='/sys/class/backlight/rpi_backlight/bl_power'
DEFAULT_TOUCH_DEVICE='/dev/input/event1'
DEFAULT_SCREEN_ACTIVATION_PERIOD = 10
DEFAULT_STREAM_IP = '0.0.0.0'
DEFAULT_STREAM_PORT = 8080

class Config(dict):

    logger = Logger('doorcam.config')

    def __init__(self, path, *args, **kwargs):
        self.logger.debug('Intializing config from file at {path}')
        super().__init__(*args, **kwargs)
        self.path = path
        self.load_defaults()
        if os.path.isfile(path):
            self.load()
        else:
            self.save()
            self.init_constants()
        self.logger.debug('Config from file {path} has been initialized!')

    def init_constants(self):
        self.logger.debug('Intializing constants from file at {path}')
        self['camera']['fourcc'] = string_to_fourcc(self['camera']['format'])
        self['camera']['resolution'] = rstring_to_rtuple(self['camera']['resolution'])
        if self['camera']['rotation'] is None:
            self['camera']['rotation_const'] = None
        else:
            self['camera']['rotation_const'] = cstring_to_cvconstant(self['camera']['rotation'])
        if type(self['camera']['K']) == str:
            self['camera']['K'] = yaml.safe_load(self['camera']['K'])
        self['camera']['K'] = np.array(self['camera']['K'])
        if type(self['camera']['D']) == str:
            self['camera']['D'] = yaml.safe_load(self['camera']['D'])
        self['camera']['D'] = np.array(self['camera']['D'])
        self['screen']['resolution'] = rstring_to_rtuple(self['screen']['resolution'])
        if self['screen']['rotation'] is None:
            self['screen']['rotation_const'] = None
        else:
            self['screen']['rotation_const'] = cstring_to_cvconstant(self['screen']['rotation'])
        self['screen']['color_conv_const'] = cstring_to_cvconstant(self['screen']['color_conv'])
        self['screen']['dtype_np'] = string_to_dtype(self['screen']['dtype'])
        self.logger.debug('Constants from file {path} has been initialized!')

    def clear_constants(self):
        del self['camera']['fourcc']
        self['camera']['resolution'] = rtuple_to_rstring(self['camera']['resolution'])
        del self['camera']['rotation_const']
        self['camera']['K'] = str(self['camera']['K'].tolist())
        self['camera']['D'] = str(self['camera']['D'].tolist())
        self['screen']['resolution'] = rtuple_to_rstring(self['screen']['resolution'])
        del self['screen']['rotation_const']
        del self['screen']['color_conv_const']
        del self['screen']['dtype_np']
        self.logger.debug('Constants from file {path} has been cleared!')


    def load(self):
        with open(self.path, 'r') as stream:
            try:
                self.update(yaml.safe_load(stream).copy())
            except yaml.YAMLError as e:
                self.logger.error(e)
        self.logger.debug(f'Loaded config from {self.path}')
        self.init_constants()
    
    def load_defaults(self):
        analysis_configs = {
            'delta_threshold': DEFAULT_ANALYSIS_DELTA_THRESHOLD,
            'contour_minimum_area': DEFAULT_ANALYSIS_CONTOUR_MIN_AREA,
            'max_fps': DEFAULT_ANALYSIS_MAX_FPS,
            'undistort': DEFAULT_ANALYSIS_UNDISTORT,
            'undistort_balance': DEFAULT_ANALYSIS_UNDISTORT_BALANCE
        }
        self.setdefault('analyzer', analysis_configs)
        camera_configs = {
            'index': DEFAULT_CAMERA_INDEX,
            'format': DEFAULT_CAMERA_FORMAT,
            'resolution': DEFAULT_CAMERA_RESOLUTION,
            'rotation': DEFAULT_CAMERA_ROTATION,
            'max_fps': DEFAULT_CAMERA_MAX_FPS,
            'K': DEFAULT_CAMERA_K,
            'D': DEFAULT_CAMERA_D,
        }
        self.setdefault('camera', camera_configs)
        screen_configs = {
            'framebuffer_device': DEFAULT_FRAMEBUFFER_DEVICE,
            'dtype': DEFAULT_FRAMEBUFFER_DTYPE,
            'resolution': DEFAULT_FRAMEBUFFER_RESOLUTION,
            'rotation': DEFAULT_FRAMEBUFFER_ROTATION,
            'backlight_device': DEFAULT_BACKLIGHT_DEVICE,
            'touch_device': DEFAULT_TOUCH_DEVICE,
            'color_conv': DEFAULT_FRAMEBUFFER_COLOR_CONV,
            'activation_period': DEFAULT_SCREEN_ACTIVATION_PERIOD,
            'undistort': DEFAULT_FRAMEBUFFER_UNDISTORT,
            'undistort_balance': DEFAULT_FRAMEBUFFER_UNDISTORT_BALANCE
        }
        self.setdefault('screen', screen_configs)
        stream_configs = {
            'ip': DEFAULT_STREAM_IP,
            'port': DEFAULT_STREAM_PORT
        }
        self.setdefault('stream', stream_configs)
    
    def save(self):
        with open(self.path, 'w') as stream:
            try:
                stream.write(yaml.safe_dump(self.copy()))
            except yaml.YAMLError as e:
                self.logger.error(e)
            self.logger.debug(f'Saved config to {self.path}')
    
def rstring_to_rtuple(resolution:str):
    resolution = resolution.lower()
    resolution = resolution.split('x')
    if len(resolution) != 2:
        raise ImproperResolutionString
    else:
        return tuple([int(x) for x in resolution])

def rtuple_to_rstring(resolution:tuple):
    if len(resolution) != 2 or any([type(x) != int for x in resolution]):
        raise ImproperResolutionTuple
    else:
        return 'x'.join([str(x) for x in resolution])

def cstring_to_cvconstant(constant:str):
    try:
        return eval(f'cv2.{constant.upper()}')
    except AttributeError as e:
        raise(ImproperCVConstant(e))

def string_to_fourcc(format:str):
    if len(format) != 4:
        raise ImproperFourCCString
    else:
        return cv2.VideoWriter_fourcc(*format)

def string_to_dtype(dtype:int):
    try:
        return(eval(f'np.{dtype.lower()}'))
    except AttributeError as e:
        raise ImproperNPDType(e)

class ImproperResolutionString(Exception):
    pass

class ImproperResolutionTuple(Exception):
    pass

class ImproperCVConstant(Exception):
    pass

class ImproperNPDType(Exception):
    pass

class ImproperFourCCString(Exception):
    pass