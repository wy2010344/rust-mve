//! 应用状态：全部用信号承载，事件只写信号，绘制期读取自动追踪。

use wy_signal::Signal;

#[derive(Clone, PartialEq)]
pub struct Todo {
    pub id: u32,
    pub text: String,
    pub done: bool,
}

pub struct App {
    pub theme_dark: Signal<bool>,
    pub name: Signal<String>,
    pub email: Signal<String>,
    pub message: Signal<String>,
    pub status: Signal<String>,
    pub todos: Signal<Vec<Todo>>,
    pub next_id: Signal<u32>,
    pub visits: Signal<u32>,
}

impl App {
    pub fn new() -> Self {
        Self {
            theme_dark: Signal::new(false),
            name: Signal::new(String::new()),
            email: Signal::new(String::new()),
            message: Signal::new(String::new()),
            status: Signal::new(String::from("填写表单后提交")),
            todos: Signal::new(vec![
                Todo {
                    id: 1,
                    text: String::from("设计系统 tokens"),
                    done: true,
                },
                Todo {
                    id: 2,
                    text: String::from("rsx 宏记录式参数"),
                    done: true,
                },
                Todo {
                    id: 3,
                    text: String::from("shadcn 仪表盘示例"),
                    done: false,
                },
            ]),
            next_id: Signal::new(4),
            visits: Signal::new(128),
        }
    }
}
