# doorcam
Raspberry Pi Peephole Camera

## Installation
```
git clone https://github.com/retrontology/doorcam
cd doorcam
python3 -m pip install -r requirements.txt
```

## Config
- <b>analyzer</b>:
  - <b>contour_minimum_area</b>: Minimum contour area of difference between frames of the analyzer to trigger a detection event.
  - <b>delta_threshold</b>: Threshold setting passed to threshold command for detecting difference between frames of tha analyzer
  - <b>max_fps</b>: Maximum desired fps. Minium fps relies on speed of single thread
  - <b>undistort</b>: Whether you want to undistort the camera image before analayzing it or not.
  - <b>undistort_balance</b>: The balance used for the undistort function if enabled
- <b>camera</b>:
  - <b>D</b>: Array of distortion coeffecients for applying fisheye undistortion. Obtained via the `calibrate.py` program.
  - <b>K</b>: Camera intrinsic matrix. Obtained via the `calibrate.py` program.
  - <b>format</b>: A four letter string used for setting the format of the capture device.
  - <b>index</b>: Index of the video device to be used for capture. i.e. if you want to use /dev/video2, your index would be 2
  - <b>max_fps</b>: Desired capture fps for the video device
  - <b>resolution</b>: Desired capture resolution for the video device
  - <b>rotation</b>: Rotation desired for frames retrieved from the video device. Is very intensive and can reduce fps if not None/null
- <b>capture</b>:
  - <b>enable</b>: Whether or not to enable saving events to disk
  - <b>keep_images</b> Whether or not to keep saved images
  - <b>path</b>: Where the images will be saved
  - <b>postroll</b>: Amount of time in seconds to capture after the last frame where motion is detected
  - <b>preroll</b>: Amount of time in seconds to capture before the first frame where motion is detected
  - <b>rotation</b>: The desired rotation to apply to the frames during post-processing.
  - <b>timestamp</b>: Whether or not to add timestamps to the saved images
  - <b>video_encode</b>: Whether or not to encode the saved images to a video file
- <b>screen</b>:
  - <b>activation_period</b>: How long in seconds you want the screen to activate for when either motion is detected or you touch the screen.
  - <b>backlight_device</b>: Path to the backlight device
  - <b>color_conv</b>: Color conversion to use for rendering to the framebuffer. Refer to https://docs.opencv.org/4.5.3/d8/d01/group__imgproc__color__conversions.html
  - <b>dtype</b>: The dtype to use for determining the width of each framebuffer pixel. Refer to https://numpy.org/doc/stable/reference/arrays.scalars.html#sized-aliases
  - <b>framebuffer_device</b>: Path to the framebuffer device to use for display.
  - <b>resolution</b>: The resolution of the framebuffer for resizing the frame for display.
  - <b>rotation</b>: The desired rotation to apply to the frame retrieved from the camera. Is significantly faster in this application as the image used is 1/4 the size of the original
  - <b>touch_device</b>: Path to the touchscreen device
  - <b>undistort</b>: Whether to undistort the frame on the screen
  - <b>undistort_balance</b>: The balance to pass to the undistortion function
- <b>stream</b>:
  - <b>ip</b>: The IP address of the desired network device to use for the MJPG server
  - <b>port</b>: The port to listen on for the MJPG server

## Usage
```
usage: run.py [-h] [-c config.yaml] [-d] [-f]

optional arguments:
  -h, --help            show this help message and exit
  -c config.yaml, --config config.yaml
  -d, --debug
  -f, --fps
```