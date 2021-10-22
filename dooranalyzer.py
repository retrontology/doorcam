from threading import Thread
from doorscreen import *
from doorcam import *
import time

DECODE_FLAGS = cv2.IMREAD_GRAYSCALE
DEFAULT_DELTA_THRESHOLD = 5
DEFAULT_CONTOUR_MIN_AREA = 5000
DEFAULT_ANALYSIS_FPS = 5

class Analyzer():

    def __init__(self, cam: Camera, screen: Screen = None, max_fps=DEFAULT_ANALYSIS_FPS, delta_threshold=DEFAULT_DELTA_THRESHOLD, contour_min_area=DEFAULT_CONTOUR_MIN_AREA):
        self.cam = cam
        self.screen = screen
        self.delta_threshold = delta_threshold
        self.contour_min_area=contour_min_area
        self.frame_count = 0
        self.fps = 0
        self.max_fps = max_fps
        self.analysis_fps_thread = Thread(target=self.analysis_fps_loop, daemon=True)
        self.analysis_fps_thread.start()
        self.analysis_thread = Thread(target=self.analysis_loop, daemon=True)
        self.analysis_thread.start()

    def analysis_loop(self):
        frame_average = None
        interval = 1.0/self.max_fps
        checkpoint = time.time()
        while True:
            try:
                frame = cv2.imdecode(self.cam.current_jpg, DECODE_FLAGS)
            except Exception as e:
                print(e)
                time.sleep(1)
                continue
            try:
                frame = cv2.GaussianBlur(frame, (21,21), 0)
            except Exception as e:
                print(e)
                continue
            if frame_average is None:
                frame_average = frame.copy().astype('float')
            cv2.accumulateWeighted(frame, frame_average, 0.5)
            frame_delta = cv2.absdiff(frame, cv2.convertScaleAbs(frame_average))
            ret, frame_threshold = cv2.threshold(frame_delta, self.delta_threshold, 255, cv2.THRESH_BINARY)
            frame_threshold = cv2.dilate(frame_threshold, None, iterations=2)
            contours, hierarchy = cv2.findContours(frame_threshold.copy(), cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
            activate = False
            for contour in contours:
                if cv2.contourArea(contour) > self.contour_min_area:
                    activate = True
            if activate and self.screen:
                self.screen.play_camera()
            self.frame_count += 1
            now = time.time()
            while(now - checkpoint < interval):
                time.sleep(0.001)
                now = time.time()
            checkpoint = now
    
    def analysis_fps_loop(self):
        checkpoint = time.time()
        while True:
            self.fps = self.frame_count
            self.frame_count = 0
            now = time.time()
            while now - checkpoint < 1:
                time.sleep(0.1)
                now = time.time()
            checkpoint = now
