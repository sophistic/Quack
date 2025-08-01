#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use enigo::{Enigo, MouseControllable};

use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

// Windows API imports
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::OpenProcess;
use winapi::um::psapi::GetModuleBaseNameW;
use winapi::um::winnt::{PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use winapi::um::winuser::{GetForegroundWindow, GetWindowThreadProcessId};

fn get_active_process_name() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }

        let mut process_id: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut process_id);

        if process_id == 0 {
            return None;
        }

        let process_handle =
            OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, process_id);

        if process_handle.is_null() {
            return None;
        }

        let mut buffer: [u16; 512] = [0; 512];
        let len = GetModuleBaseNameW(
            process_handle,
            ptr::null_mut(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        );

        CloseHandle(process_handle);

        if len > 0 {
            let os_string = OsString::from_wide(&buffer[..len as usize]);
            if let Ok(process_name) = os_string.into_string() {
                // Remove .exe extension if present
                if process_name.ends_with(".exe") {
                    Some(process_name[..process_name.len() - 4].to_string())
                } else {
                    Some(process_name)
                }
            } else {
                None
            }
        } else {
            None
        }
    }
}

fn smooth_resize(
    window: &WebviewWindow,
    from: tauri::PhysicalSize<u32>,
    to: tauri::PhysicalSize<u32>,
    steps: u32,
    delay: u64,
) {
    if steps == 0 {
        let _ = window.set_size(tauri::Size::Physical(to));
        return;
    }

    let step_width = (to.width as i32 - from.width as i32) / steps as i32;
    let step_height = (to.height as i32 - from.height as i32) / steps as i32;

    for i in 1..=steps {
        let new_width = from.width as i32 + step_width * i as i32;
        let new_height = from.height as i32 + step_height * i as i32;

        // Setting the new size, ensuring the dimensions are not less than 1.
        let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize {
            width: new_width.max(1) as u32,
            height: new_height.max(1) as u32,
        }));

        // Wait for a short duration to create the animation effect.
        thread::sleep(Duration::from_millis(delay));
    }
    // Ensure the final size is exactly the target size as defined in the tauri.conf.json file
    let _ = window.set_size(tauri::Size::Physical(to));
}

fn smooth_move(
    window: &WebviewWindow,
    from: tauri::PhysicalPosition<i32>,
    to: tauri::PhysicalPosition<i32>,
    steps: u32,
    delay: u64,
) {
    if steps == 0 {
        let _ = window.set_position(tauri::Position::Physical(to));
        return;
    }

    let dx = (to.x - from.x) / steps as i32;
    let dy = (to.y - from.y) / steps as i32;

    for i in 1..=steps {
        let new_x = from.x + dx * i as i32;
        let new_y = from.y + dy * i as i32;

        let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
            x: new_x,
            y: new_y,
        }));

        thread::sleep(Duration::from_millis(delay));
    }

    // Ensure final position is accurate
    let _ = window.set_position(tauri::Position::Physical(to));
}

#[tauri::command]
fn follow_magic_dot(app: AppHandle) {
    let Some(window) = app.get_webview_window("magic-dot") else {
        println!("Magic-dot window not found");
        return;
    };

    // Get the window's current size to animate from.
    let current_size = window.outer_size().unwrap();

    // Animate the window shrinking into a small "dot".
    smooth_resize(
        &window,
        current_size,
        tauri::PhysicalSize {
            width: 20,
            height: 20,
        },
        10, // steps
        10, // delay in ms
    );

    // Spawn a new thread to handle the mouse-following logic,
    // so the main thread is not blocked.
    thread::spawn(move || {
        let enigo = Enigo::new();

        // Define the constant original size to restore to.
        let original_size = tauri::PhysicalSize {
            width: 400,
            height: 48,
        };

        // Loop for indefinitely to track the mouse.
        loop {
            // Get the current mouse cursor position on the screen.
            let (mouse_x, mouse_y) = enigo.mouse_location();

            // Get the window's current position.
            if let Ok(position) = window.outer_position() {
                // Calculate the center of the "dot" window.
                let window_center_x = position.x + 10; // 10 is half of the dot's width (20)
                let window_center_y = position.y + 10; // 10 is half of the dot's height (20)

                // Calculate the vector and distance from the window center to the mouse.
                let dx = mouse_x - window_center_x;
                let dy = mouse_y - window_center_y;
                let distance = ((dx * dx + dy * dy) as f64).sqrt();
                println!("Distance to mouse: {}", distance);
                // If the mouse gets very close to the dot, exit follow mode.
                if distance < 20.0 {
                    // Emit an event to the frontend to signal the exit.

                    let current_dot_size = window.outer_size().unwrap_or(tauri::PhysicalSize {
                        width: 10,
                        height: 10,
                    });

                    // Animate the window expanding back to its original size.
                    smooth_resize(&window, current_dot_size, original_size, 10, 10);
                    println!("Emitting exit_follow_mode");
                    let _ = app.emit("exit_follow_mode", ());
                    println!("Emitting onboarding_done");
                    let _ = app.emit("onboarding_done", ());
                    // Break the loop to stop following the mouse.
                    break;
                }

                // If the mouse is a certain distance away, move the dot towards it.
                // This creates a "lag" or "spring" effect.
                if distance > 40.0 {
                    let new_x = position.x + ((dx as f64) * 0.15) as i32;
                    let new_y = position.y + ((dy as f64) * 0.15) as i32;

                    // Set the window's new position.
                    let _ =
                        window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                            x: new_x,
                            y: new_y,
                        }));
                }
            }

            // Sleep for ~16ms to target roughly 60 updates per second.
            thread::sleep(Duration::from_millis(4));
        }
    });
}

#[tauri::command]
fn pin_magic_dot(app: AppHandle) {
    if let Some(window) = app.get_webview_window("magic-dot") {
        if let (Ok(current_pos), Ok(current_size), Ok(Some(monitor))) = (
            window.outer_position(),
            window.outer_size(),
            window.current_monitor(),
        ) {
            let screen_size = monitor.size();

            let center_x = ((screen_size.width as i32 - current_size.width as i32) / 2).max(0);
            let target_pos = tauri::PhysicalPosition { x: center_x, y: 0 };

            // Smoothly move the window to the top-center of the screen
            smooth_move(&window, current_pos, target_pos, 10, 10);

            println!("Pinned magic dot to top-center");
        }
    } else {
        println!("magic-dot window not found");
    }
}

#[tauri::command]
fn start_window_watch(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_title = String::new();

        loop {
            if let Some(process_name) = get_active_process_name() {
                if process_name != last_title && !process_name.is_empty() && process_name != "quack"
                {
                    if let Some(magic_dot_window) = app.get_webview_window("magic-dot") {
                        let _ =
                            magic_dot_window.emit("active_window_changed", process_name.clone());
                        println!("Emitted process name: {}", process_name);
                    }
                    last_title = process_name;
                }
            }

            // Poll every 1 second
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
}

#[tauri::command] // to close the winodw when onboarding is finished
fn close_onboarding_window(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.close();
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            follow_magic_dot,
            pin_magic_dot,
            start_window_watch,
            close_onboarding_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}


