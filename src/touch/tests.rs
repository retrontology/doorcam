use super::*;
use crate::config::DisplayConfig;
use evdev::{AbsoluteAxisType, EventType, InputEvent, Key};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

fn create_test_config() -> DisplayConfig {
    DisplayConfig {
        framebuffer_device: "/dev/fb0".to_string(),
        backlight_device: "/sys/class/backlight/rpi_backlight/brightness".to_string(),
        touch_device: "/dev/input/event0".to_string(),
        activation_period_seconds: 30,
        resolution: (800, 480),
        rotation: None,
    }
}

#[tokio::test]
async fn test_mock_touch_handler() {
    let event_bus = Arc::new(crate::events::EventBus::new());
    let mut receiver = event_bus.subscribe();

    let mock_handler = MockTouchInputHandler::new(Arc::clone(&event_bus));
    mock_handler.trigger_touch().await.unwrap();

    let event = receiver.recv().await.unwrap();
    match event {
        crate::events::DoorcamEvent::TouchDetected { .. } => {}
        _ => panic!("Expected TouchDetected event"),
    }
}

#[test]
fn test_touch_event_creation() {
    let timestamp = SystemTime::now();
    let touch_event = TouchEvent::press(timestamp);

    assert_eq!(touch_event.event_type, TouchEventType::Press);
    assert_eq!(touch_event.x, None);
    assert_eq!(touch_event.y, None);
    assert_eq!(touch_event.timestamp, timestamp);
}

#[test]
fn test_touch_event_with_coordinates() {
    let timestamp = SystemTime::now();
    let touch_event = TouchEvent::press_at(100, 200, timestamp);

    assert_eq!(touch_event.event_type, TouchEventType::Press);
    assert_eq!(touch_event.x, Some(100));
    assert_eq!(touch_event.y, Some(200));
    assert_eq!(touch_event.timestamp, timestamp);
}

#[test]
fn test_is_touch_event() {
    let press_event = InputEvent::new(EventType::KEY, Key::BTN_TOUCH.code(), 1);
    assert!(TouchInputHandler::is_touch_event(&press_event));

    let release_event = InputEvent::new(EventType::KEY, Key::BTN_TOUCH.code(), 0);
    assert!(!TouchInputHandler::is_touch_event(&release_event));

    let other_key_event = InputEvent::new(EventType::KEY, Key::KEY_A.code(), 1);
    assert!(!TouchInputHandler::is_touch_event(&other_key_event));
}

#[tokio::test]
async fn test_touch_handler_creation() {
    let config = create_test_config();
    let event_bus = Arc::new(crate::events::EventBus::new());

    let handler = TouchInputHandler::new(&config, Arc::clone(&event_bus));
    assert_eq!(handler.device_path, "/dev/input/event0");
}

#[tokio::test]
async fn test_advanced_touch_handler_creation() {
    let config = create_test_config();
    let event_bus = Arc::new(crate::events::EventBus::new());

    let mut handler = AdvancedTouchInputHandler::new(&config, Arc::clone(&event_bus));
    handler.set_debounce_duration(std::time::Duration::from_millis(100));

    assert_eq!(handler.device_path, "/dev/input/event0");
    assert_eq!(handler.debounce_duration, std::time::Duration::from_millis(100));
}

#[test]
fn test_parse_advanced_touch_event() {
    let current_position = Arc::new(RwLock::new((None, None)));

    let press_event = InputEvent::new(EventType::KEY, Key::BTN_TOUCH.code(), 1);

    let touch_event =
        AdvancedTouchInputHandler::parse_advanced_touch_event(&press_event, &current_position);
    assert!(touch_event.is_some());

    let event = touch_event.unwrap();
    assert_eq!(event.event_type, TouchEventType::Press);

    let x_event = InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_X.0, 100);

    let coord_event =
        AdvancedTouchInputHandler::parse_advanced_touch_event(&x_event, &current_position);
    assert!(coord_event.is_none());

    let pos = current_position.read().unwrap();
    assert_eq!(pos.0, Some(100));
}

