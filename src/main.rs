// build command:
// cargo +nightly build --release

#![feature(decl_macro,proc_macro_hygiene)]

#[macro_use] extern crate rocket;

use std::io::prelude::*;
use std::sync::mpsc::{sync_channel, SyncSender, Receiver};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use rocket::request::Form;
use rocket::State;
use spidev::{Spidev, SpidevOptions, SpiModeFlags};

struct LedSender {
    sender: SyncSender<LedState>,
}

struct LedState {
    color1: ColorHsv,
    color2: ColorHsv,
    color3: ColorHsv,
    pattern: u8,
}

#[derive(FromForm)]
struct LedStateData {
    color1_h: f32,
    color1_s: f32,
    color1_v: f32,
    color2_h: f32,
    color2_s: f32,
    color2_v: f32,
    color3_h: f32,
    color3_s: f32,
    color3_v: f32,
    pattern: u8,
}

struct Pixel {
    r: u8,
    g: u8,
    b: u8,
}

/*
struct ColorRgb {
    r: u8,
    g: u8,
    b: u8,
}
*/

struct ColorHsv {
    h: f32,
    s: f32,
    v: f32,
}

//~ struct EventLoop {
    //~ inst: Instant,
//~ }

fn write_leds(spi: &mut Spidev, leds: &Vec<Pixel>, offset: usize) {
    let mut tmp = Vec::new();
    for _i in 0..4 {
        tmp.push(0);
    }
    
    //~ for led in leds.iter() {
        //~ tmp.push(255);
        //~ tmp.push(led.b);
        //~ tmp.push(led.g);
        //~ tmp.push(led.r);
    //~ }
    
    for i in offset..(offset + leds.len()) {
        let i_wrap = i % leds.len();
        tmp.push(255);
        tmp.push(leds[i_wrap].b);
        tmp.push(leds[i_wrap].g);
        tmp.push(leds[i_wrap].r);
    }
    
    for _i in 0..4 {
        tmp.push(0);
    }
    spi.write(&tmp[..]).unwrap();
}

fn hsv_2_rgb(col: &ColorHsv) -> Pixel {
    let mut out = Pixel { r: 0, g: 0, b: 0 };
    match (col.h * 6.0).trunc() as u8 {
        0 => {
            out.r = (col.v * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s * (1.0 - ((col.h * 6.0) - ((col.h * 6.0).trunc()))))) * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s)) * 255.0) as u8;
        }
        1 => {
            out.r = ((col.v * (1.0 - col.s * ((col.h * 6.0) - ((col.h * 6.0).trunc())))) * 255.0) as u8;
            out.g = (col.v * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s)) * 255.0) as u8;
        }
        2 => {
            out.r = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.g = (col.v * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s * (1.0 - ((col.h * 6.0) - ((col.h * 6.0).trunc()))))) * 255.0) as u8;
        }
        3 => {
            out.r = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s * ((col.h * 6.0) - ((col.h * 6.0).trunc())))) * 255.0) as u8;
            out.b = (col.v * 255.0) as u8;
        }
        4 => {
            out.r = ((col.v * (1.0 - col.s * (1.0 - ((col.h * 6.0) - ((col.h * 6.0).trunc()))))) * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.b = (col.v * 255.0) as u8;
        }
        5 => {
            out.r = (col.v * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s * ((col.h * 6.0) - ((col.h * 6.0).trunc())))) * 255.0) as u8;
        }
        _ => (),
    }
    out
}

fn hsv_interp(col1: &ColorHsv, col2: &ColorHsv, pos: f32) -> ColorHsv {
    let h_range = col1.h - col2.h;
    let s_range = col1.s - col2.s;
    let v_range = col1.v - col2.v;
    
    let h_out = col1.h - (h_range * pos.min(1.0));
    let s_out = col1.s - (s_range * pos.min(1.0));
    let v_out = col1.v - (v_range * pos.min(1.0));
    
    ColorHsv { h: h_out, s: s_out, v: v_out }
}

fn hsv_interp_3(col1: &ColorHsv, col2: &ColorHsv, col3: &ColorHsv, pos: f32) -> ColorHsv {
    //let mut h_out = 0.0;
    //let mut s_out = 0.0;
    //let mut v_out = 0.0;
    
    if pos > 0.0 {
        let h_range = col1.h - col2.h;
        let s_range = col1.s - col2.s;
        let v_range = col1.v - col2.v;
        
        let h_out = col1.h - (h_range * pos.min(1.0));
        let s_out = col1.s - (s_range * pos.min(1.0));
        let v_out = col1.v - (v_range * pos.min(1.0));

        ColorHsv { h: h_out, s: s_out, v: v_out }
    } else if pos < 0.0 {
        let h_range = col1.h - col3.h;
        let s_range = col1.s - col3.s;
        let v_range = col1.v - col3.v;
        
        let h_out = col1.h - (h_range * (pos * -1.0).min(1.0));
        let s_out = col1.s - (s_range * (pos * -1.0).min(1.0));
        let v_out = col1.v - (v_range * (pos * -1.0).min(1.0));

        ColorHsv { h: h_out, s: s_out, v: v_out }
    } else {
        let h_out = col1.h;
        let s_out = col1.s;
        let v_out = col1.v;

        ColorHsv { h: h_out, s: s_out, v: v_out }
    }
    
    //ColorHsv { h: h_out, s: s_out, v: v_out }
}

fn triangle(pos: f32, phs: f32) -> f32 {
    ((pos + phs).rem_euclid(1.0) * 2.0 - 1.0).abs() * 2.0 - 1.0
}

#[get("/")]
fn index() -> &'static str {
    //~ led_sender.sender.send(LedState {
        //~ color1: Pixel { r: 63, g: 0, b: 0 },
        //~ color2: Pixel { r: 0, g: 63, b: 0 },
        //~ color3: Pixel { r: 0, g: 0, b: 63 },
        //~ pattern: 1 }).unwrap();
    "foo bar"
}

/*
#[get("/off")]
fn off(led_sender: State<LedSender>) {
    //~ led_sender.sender.send(LedState {
        //~ color1: Pixel { r: 63, g: 0, b: 0 },
        //~ color2: Pixel { r: 0, g: 63, b: 0 },
        //~ color3: Pixel { r: 0, g: 0, b: 63 },
        //~ pattern: 0 }).unwrap();
}
*/

#[post("/set", data = "<data>")]
fn set(led_sender: State<LedSender>, data: Form<LedStateData>) {
    led_sender.sender.send(LedState {
        color1: ColorHsv { h: data.color1_h, s: data.color1_s, v: data.color1_v },
        color2: ColorHsv { h: data.color2_h, s: data.color2_s, v: data.color2_v },
        color3: ColorHsv { h: data.color3_h, s: data.color3_s, v: data.color3_v },
        pattern: data.pattern }).unwrap();
}

fn main() {
    let (tx, rx): (SyncSender<LedState>, Receiver<LedState>) = sync_channel(1);
    thread::spawn(move || {
        let mut spidev = Spidev::open("/dev/spidev0.0").unwrap();
        let options = SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(8_000)
            .mode(SpiModeFlags::SPI_MODE_0)
            .build();
        spidev.configure(&options).unwrap();

        let led_count: usize = 12;
        
        let mut leds_1 = Vec::with_capacity(led_count);
        //~ leds_1.push(Pixel { r: 63, g: 0, b: 0 });
        //~ leds_1.push(Pixel { r: 0, g: 63, b: 0 });
        //~ leds_1.push(Pixel { r: 0, g: 0, b: 63 });
        for _i in 0..led_count {
            leds_1.push(Pixel { r: 0, g: 0, b: 0 });
        }

        let mut leds_off = Vec::with_capacity(led_count);
        //~ leds_off.push(Pixel { r: 0, g: 0, b: 0 });
        //~ leds_off.push(Pixel { r: 0, g: 0, b: 0 });
        //~ leds_off.push(Pixel { r: 0, g: 0, b: 0 });
        for _i in 0..led_count {
            leds_off.push(Pixel { r: 0, g: 0, b: 0 });
        }
        
        let mut offset: f32 = 0.0;
        let offset_inc = 0.005;
        //~ let mut offset_count: u8 = 0;
        //~ let offset_frames: u8 = 5;
        
        let loop_time: f32 = 16.6666;
        let mut pattern: u8 = 0;
        
        let mut color1 = ColorHsv {h: 0.0, s: 0.0, v: 0.0 };
        let mut color2 = ColorHsv {h: 0.0, s: 0.0, v: 0.0 };
        let mut color3 = ColorHsv {h: 0.0, s: 0.0, v: 0.0 };

        loop {
            let loop_start = Instant::now();
            let msg = rx.try_recv();

            match msg {
                Ok(msg) => {
                    match msg.pattern {
                        0 => {
                            write_leds(&mut spidev, &leds_off, 0);
                            pattern = msg.pattern;
                        }
                        1 => {
                            //~ leds_1[0] = hsv_2_rgb(&msg.color1);
                            //~ leds_1[1] = hsv_2_rgb(&msg.color2);
                            //~ leds_1[2] = hsv_2_rgb(&msg.color3);
                            //~ leds_1[3] = hsv_2_rgb(&msg.color1);
                            //~ leds_1[4] = hsv_2_rgb(&msg.color2);
                            //~ leds_1[5] = hsv_2_rgb(&msg.color3);
                            //~ leds_1[6] = hsv_2_rgb(&msg.color1);
                            //~ leds_1[7] = hsv_2_rgb(&msg.color2);
                            //~ leds_1[8] = hsv_2_rgb(&msg.color3);
                            
                            color1 = msg.color1;
                            color2 = msg.color2;
                            color3 = msg.color3;
                                
                            for (i, led) in leds_1.iter_mut().enumerate() {
                                
                                let pos = (i as f32) / ((led_count - 1) as f32);
                                //~ if pos < 0.5 {
                                    //~ *led = hsv_2_rgb(&hsv_interp(&msg.color1, &msg.color2, pos * 2.0));
                                //~ } else {
                                    //~ *led = hsv_2_rgb(&hsv_interp(&msg.color2, &msg.color3, pos * 2.0 - 1.0));
                                //~ }
                                
                                //~ if pos < 0.3333334 {
                                    //~ *led = hsv_2_rgb(&hsv_interp(&msg.color1, &msg.color2, pos * 3.0));
                                //~ } else if pos > 0.33333334 && pos < 0.66666667 {
                                    //~ *led = hsv_2_rgb(&hsv_interp(&msg.color2, &msg.color3, pos * 3.0 - 1.0));
                                //~ } else {
                                    //~ *led = hsv_2_rgb(&hsv_interp(&msg.color3, &msg.color1, pos * 3.0 - 2.0));
                                //~ }
                                
                                /*
                                if pos <= 0.25 || pos >= 0.75 {
                                    *led = hsv_2_rgb(&hsv_interp(&color2, &color1, triangle(0.0 + offset, pos)));
                                    //~ *led = Pixel { r: 0, g: 0, b: 0 };
                                } else {
                                    *led = hsv_2_rgb(&hsv_interp(&color3, &color1, triangle(1.0 - offset, -pos)));
                                    //~ *led = Pixel { r: 0, g: 0, b: 0 };
                                }
                                */
                                
                                *led = hsv_2_rgb(&hsv_interp_3(&color1, &color2, &color3, triangle(0.0 + offset, pos)));
                            }
                            
                            write_leds(&mut spidev, &leds_1, 0);
                            pattern = msg.pattern;
                        }
                        _ => (),
                    }
                }
                Err(_) => (),
            }
            
            match pattern {
                0 => (),
                1 => {
                    for (i, led) in leds_1.iter_mut().enumerate() {
                        let pos = (i as f32) / ((led_count - 1) as f32);
                        /*
                        if pos <= 0.25 || pos >= 0.75 {
                            *led = hsv_2_rgb(&hsv_interp(&color2, &color1, triangle(0.0 + offset, pos)));
                            //~ *led = Pixel { r: 0, g: 0, b: 0 };
                        } else {
                            *led = hsv_2_rgb(&hsv_interp(&color3, &color1, triangle(1.0 - offset, -pos)));
                            //~ *led = Pixel { r: 0, g: 0, b: 0 };
                        }
                        */
                        
                        /*
                        if pos < 0.5 {
                            //~ *led = hsv_2_rgb(&hsv_interp(&color1, &color2, triangle(0.0 + offset, (pos + 0.75).rem_euclid(1.0))));
                            *led = Pixel { r: 0, g: 0, b: 0 };
                        } else {
                            *led = hsv_2_rgb(&hsv_interp(&color3, &color1, triangle(0.0 + offset, (pos + 0.75).rem_euclid(1.0)) * -1.0));
                            //~ *led = Pixel { r: 0, g: 0, b: 0 };
                        }
                        */
                        
                        *led = hsv_2_rgb(&hsv_interp_3(&color1, &color2, &color3, triangle(0.0 + offset, pos)));
                    }
                    write_leds(&mut spidev, &leds_1, 0);
                }
                _ => (),
            }
            
            //~ offset_count += 1;
            //~ if offset_count >= (offset_frames - 1) {
                //~ offset_count = 0;
                //~ offset += 1;
                //~ if offset > (leds_1.len() - 1) {
                    //~ offset = 0;
                //~ }
            //~ }
            
            offset = (offset + offset_inc).rem_euclid(1.0);
            
            let loop_time_adj = (loop_time - (loop_start.elapsed().as_millis() as f32)) / 1000.0;
            if (loop_start.elapsed().as_millis() as f32) < loop_time {
                sleep(Duration::from_secs_f32(loop_time_adj));
            }
            //~ sleep(Duration::from_secs_f32(loop_time / 1000.0));
        }
        //write_leds(&mut spidev, &leds_1);
        
        //loop {
        //    println!("{}", rx.recv().unwrap());
        //}
    });

    rocket::ignite().manage(LedSender { sender: tx }).mount("/", routes![index, set]).launch();
}
