from ast import parse
import cv2
import numpy as np
from threading import Thread
from doorscreen import *
from doorcam import *
from doorstream import *
from dooranalyzer import *
from doorconfig import *
import time
from functools import partial
import argparse

def setup_logger(debug=False):
    pass

def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument('-c', '--config', default=os.path.join(os.path.dirname('__file__'), 'config.yaml'), metavar='config.yaml')
    parser.add_argument('-d', '--debug', action='store_true')
    return parser.parse_args()

def main():
    args = parse_args()
    config = Config(args.config)
    logger = setup_logger(args.debug)
    cam = Camera(
        config['camera']['index'], 
        config['camera']['resolution'], 
        config['camera']['rotation_const'], 
        config['camera']['max_fps'], 
        config['camera']['fourcc'], 
        config['camera']['K'], 
        config['camera']['D']
    )
    screen = Screen(
        cam, 
        config['screen']['resolution'], 
        config['screen']['rotation_const'], 
        config['screen']['framebuffer_device'], 
        config['screen']['backlight_device'], 
        config['screen']['touch_device'], 
        config['screen']['color_conv_const'], 
        config['screen']['dtype_np'], 
        config['screen']['activation_period'], 
        config['screen']['undistort'], 
        config['screen']['undistort_balance']
    )
    screen.play_camera()
    analyzer = Analyzer(
        cam,
        screen,
        config['analyzer']['max_fps'],
        config['analyzer']['delta_threshold'],
        config['analyzer']['contour_minimum_area'],
        config['analyzer']['undistort'],
        config['analyzer']['undistort_balance']
    )
    stream_handler = partial(MJPGHandler, cam)
    server = MJPGServer((config['stream']['ip'], config['stream']['port']), stream_handler)
    #http_thread = Thread(target=server.serve_forever, daemon=True)
    #http_thread.start()
    server.serve_forever()
    #while True:
    #    print(f'Cam: {cam.fps} | Screen: {screen.fps} | Analyzer: {analyzer.fps}')
    #    time.sleep(1)

if __name__ == '__main__':
    main()