from threading import Thread
from doorscreen import *
from doorcam import *
import time

DECODE_FLAGS = cv2.IMREAD_GRAYSCALE
DEFAULT_DELTA_THRESHOLD = 5
DEFAULT_CONTOUR_MIN_AREA = 5000
DEFAULT_ANALYSIS_FPS = 5

class Analyzer():

    def __init__(self, cam: Camera, screen: Screen = None, max_fps=DEFAULT_ANALYSIS_FPS, delta_threshold=DEFAULT_DELTA_THRESHOLD, contour_min_area=DEFAULT_CONTOUR_MIN_AREA, undistort=True):
        self.camera = cam
        self.screen = screen
        self.delta_threshold = delta_threshold
        self.contour_min_area=contour_min_area
        self.frame_count = 0
        self.fps = 0
        self.max_fps = max_fps
        self.setup_undistort(undistort)
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
                frame = cv2.imdecode(self.camera.current_jpg, DECODE_FLAGS)
            except Exception as e:
                print(e)
                time.sleep(1)
                continue
            if self.undistort:
                frame = cv2.remap(frame, self.undistort_map1, self.undistort_map2, interpolation=cv2.INTER_LINEAR, borderMode=cv2.BORDER_CONSTANT)
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

    def setup_undistort(self, undistort=True):
        self.undistort = undistort
        undistort_DIM=tuple([int(x/4) for x in self.camera.resolution])
        if type(self.camera.undistort_K) is np.ndarray:
            undistort_K = self.camera.undistort_K/4
            undistort_K[2][2] = 1.0
        else:
            undistort_K=np.array([[undistort_DIM[1]/2, 0, undistort_DIM[0]/2], [0, undistort_DIM[1]/2, undistort_DIM[1]/2], [0, 0, 1]])
        if type(self.camera.undistort_D) is np.ndarray:
            undistort_D = self.camera.undistort_D
        else:
            undistort_D = np.array([0.01, -0.01, 0.01, -0.01])
        if type(self.camera.undistort_NK) is np.ndarray:
            undistort_NK = self.camera.undistort_NK/4
            undistort_NK[2][2] = 1.0
        else:
            undistort_NK = undistort_K.copy()
            undistort_NK[0,0] = undistort_K[0,0]/DEFAULT_K_SCALE
            undistort_NK[1,1] = undistort_K[1,1]/DEFAULT_K_SCALE
        self.undistort_map1, self.undistort_map2 = cv2.fisheye.initUndistortRectifyMap(undistort_K, undistort_D, np.eye(3), undistort_NK, undistort_DIM, cv2.CV_16SC2)