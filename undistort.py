import cv2
import numpy as np
import sys

DIM=(1920, 1080)
K=np.array([[539.8606873339231, 0.0, 999.745990731636], [0.0, 540.4889507343736, 541.3382370501859], [0.0, 0.0, 1.0]])
new_K=np.array([[197.38024030151098, 0.0, 953.7677809843199], [0.0, 197.60994174831563, 540.5796661140536], [0.0, 0.0, 1.0]])
D=np.array([[-0.06300247530706406], [0.028367414247228113], [-0.018682028009339952], [0.0037199220124150604]])

def undistort(img_path):

    img = cv2.imread(img_path)
    h,w = img.shape[:2]

    map1, map2 = cv2.fisheye.initUndistortRectifyMap(K, D, np.eye(3), K, DIM, cv2.CV_16SC2)
    undistorted_img = cv2.remap(img, map1, map2, interpolation=cv2.INTER_LINEAR, borderMode=cv2.BORDER_CONSTANT)

    cv2.imshow("undistorted", undistorted_img)
    cv2.waitKey(0)
    cv2.destroyAllWindows()

if __name__ == '__main__':
    for p in sys.argv[1:]:
        undistort(p)