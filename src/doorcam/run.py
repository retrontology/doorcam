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
from logging import getLogger, StreamHandler, Formatter, DEBUG, INFO
from cysystemd import journal
import psutil
from doorcapture import *

def setup_logger(debug=False):
    logger = getLogger('doorcam')
    stream_formatter = Formatter('%(asctime)s [%(levelname)s] %(name)s: %(message)s')
    journald_handler = journal.JournalHandler()
    stream_handler = StreamHandler()
    stream_handler.setFormatter(stream_formatter)
    if debug:
        logger.setLevel(DEBUG)
        journald_handler.setLevel(DEBUG)
        stream_handler.setLevel(DEBUG)
    else:
        logger.setLevel(INFO)
        journald_handler.setLevel(INFO)
        stream_handler.setLevel(INFO)
    if psutil.Process(os.getpid()).ppid() == 1:
        logger.addHandler(journald_handler)
    else:
        logger.addHandler(stream_handler)
    return logger

def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument('-c', '--config', default=os.path.join(os.path.dirname('__file__'), 'config.yaml'), metavar='config.yaml')
    parser.add_argument('-d', '--debug', action='store_true')
    parser.add_argument('-f', '--fps', action='store_true')
    return parser.parse_args()

def main():
    args = parse_args()
    config = Config(args.config)
    logger = setup_logger(args.debug)
    analyzer_callbacks = set()
    cam = Camera(
        config['camera']['index'], 
        config['camera']['resolution'], 
        config['camera']['rotation_const'], 
        config['camera']['max_fps'], 
        config['camera']['fourcc'], 
        config['camera']['K'], 
        config['camera']['D']
    )
    if config['screen']['enable']:
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
        analyzer_callbacks.add(screen.play_camera)
    if config['capture']['enable']:
        try:
            capture = Capture(
                cam,
                config['capture']['preroll'],
                config['capture']['postroll'],
                config['capture']['path'],
                config['capture']['timestamp'],
                config['capture']['rotation_const'],
                config['capture']['video_encode'],
                config['capture']['keep_images'],
                config['capture']['trim_old'],
                config['capture']['trim_limit'],
            )
            analyzer_callbacks.add(capture.trigger_capture)
        except Exception as e:
            logger.error(e)
    analyzer = Analyzer(
        cam,
        config['analyzer']['max_fps'],
        config['analyzer']['delta_threshold'],
        config['analyzer']['contour_minimum_area'],
        config['analyzer']['undistort'],
        config['analyzer']['undistort_balance'],
        analyzer_callbacks
    )
    stream_handler = partial(MJPGHandler, cam)
    server = MJPGServer((config['stream']['ip'], config['stream']['port']), stream_handler)
    if args.fps:
        http_thread = Thread(target=server.serve_forever, daemon=True)
        http_thread.start()
        while True:
            logger.info(f'Cam: {cam.fps} | Screen: {screen.fps} | Analyzer: {analyzer.fps}')
            time.sleep(1)
    else:
        server.serve_forever()
    

if __name__ == '__main__':
    main()