// 隐藏 release 模式下的控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 先看是不是被自己以「MSR 温度桥接」身份提权拉起的：那种情况下不启动窗口，
    // 只跑读温度的子进程循环（详见 msr_bridge.rs）。
    if pulse_deck_lib::run_bridge_if_requested() {
        return;
    }
    pulse_deck_lib::run()
}
