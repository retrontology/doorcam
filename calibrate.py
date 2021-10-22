import cv2
#assert cv2.__version__[0] == '3', 'The fisheye module requires opencv version >= 3.0.0'
import numpy as np
import os
import glob
import pickle

CHECKERBOARD = (5,7)

SUBPIX_CRITERIA = (cv2.TERM_CRITERIA_EPS+cv2.TERM_CRITERIA_MAX_ITER, 30, 0.1)
CALIBRATION_FLAGS = cv2.fisheye.CALIB_RECOMPUTE_EXTRINSIC+cv2.fisheye.CALIB_CHECK_COND+cv2.fisheye.CALIB_FIX_SKEW

OBJP = np.zeros((1, CHECKERBOARD[0]*CHECKERBOARD[1], 3), np.float32)
OBJP[0,:,:2] = np.mgrid[0:CHECKERBOARD[0], 0:CHECKERBOARD[1]].T.reshape(-1, 2)

IMAGE_DIR = '/mnt/c/Users/retrontology/Desktop/candidates3/'

def write_points(image_dir):
    images = glob.glob(os.path.join(image_dir, '*.jpg'))
    point_dir = os.path.join(image_dir, 'points')
    if not os.path.isdir(point_dir):
        os.mkdir(point_dir)
    _img_shape = None
    for fname in images:
        img = cv2.imread(fname)
        if _img_shape == None:
            _img_shape = img.shape[:2]
        else:
            assert _img_shape == img.shape[:2], "All images must share the same size."
        gray = cv2.cvtColor(img,cv2.COLOR_BGR2GRAY)
        #print(gray.shape[::-1])
        ret, corners = cv2.findChessboardCorners(gray, CHECKERBOARD, cv2.CALIB_CB_ADAPTIVE_THRESH+cv2.CALIB_CB_FAST_CHECK+cv2.CALIB_CB_NORMALIZE_IMAGE+cv2.CALIB_CB_FILTER_QUADS)
        if ret == True:
            name = os.path.basename(fname).split('.jpg')[0] + '.pts'
            cv2.cornerSubPix(gray,corners,(3,3),(-1,-1),SUBPIX_CRITERIA)
            with open(os.path.join(point_dir, name), 'wb') as f:
                pickle.dump(corners, f)
    print(f'point_dir = \'{point_dir}\'')
    print(f'_img_shape = {_img_shape}')
    print(f'gshape = {gray.shape}')
    print(f'ishape = {img.shape}')
    return (point_dir, _img_shape, gray.shape, img.shape)

def calculate_K_D(point_dir, ishape):
    img_dim = ishape[:2]
    objpoints = []
    imgpoints = []
    count = 0
    for fname in glob.glob(os.path.join(point_dir, '*.pts')):
        with open(fname, 'rb') as f:
            corners = pickle.load(f)
            objpoints.append(OBJP)
            imgpoints.append(corners)
            print(f'{count} | {fname}')
            count+=1
    K = np.zeros((3, 3))
    D = np.zeros((4, 1))
    rvecs = [np.zeros((1, 1, 3), dtype=np.float64) for i in range(count)]
    tvecs = [np.zeros((1, 1, 3), dtype=np.float64) for i in range(count)]
    retval, K, D, rvecs, tvecs = cv2.fisheye.calibrate(
        objpoints,
        imgpoints,
        img_dim,
        K,
        D,
        rvecs,
        tvecs,
        CALIBRATION_FLAGS,
        (cv2.TERM_CRITERIA_EPS+cv2.TERM_CRITERIA_MAX_ITER, 30, 1e-6))

    DIM= img_dim
    balance=1

    print(f'{K} * {img_dim[0]} / {DIM[0]} = {K * img_dim[0] / DIM[0]}')

    scaled_K = K * img_dim[0] / DIM[0]  
    scaled_K[2][2] = 1.0  
    new_K = cv2.fisheye.estimateNewCameraMatrixForUndistortRectify(scaled_K, D, DIM, np.eye(3), balance=balance)

    print("DIM=" + str(img_dim))
    print("K=np.array(" + str(K.tolist()) + ")")
    print("new_K=np.array(" + str(new_K.tolist()) + ")")
    print("scaled_K=np.array(" + str(scaled_K.tolist()) + ")")
    print("D=np.array(" + str(D.tolist()) + ")")



#point_dir, _img_shape, gshape, ishape = write_points(IMAGE_DIR)
point_dir = '/mnt/c/Users/retrontology/Desktop/candidates3/points'
ishape = (1080, 1920, 3)
calculate_K_D(point_dir, ishape)