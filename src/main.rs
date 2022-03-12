use std::io;
use std::io::Read;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use termios::*;

static EDITEUREN_VERSION: &str = "11";

struct Keyboard {
  stdin: io::Stdin
}

impl Keyboard {
  fn new() -> Keyboard {
    Keyboard { stdin: io::stdin() }
  }

  fn ctrl_key(c: char) -> char {
    (c as u8 & 0x1fu8) as char
  }

  fn read_key(&mut self) -> char {
    let mut buf: [u8; 1] = [0];
  
    loop {
      match self.stdin.read(&mut buf) {
        Ok(1)  => break buf[0] as char,
        Ok(_)  => (),
        Err(e) => panic!("Failed because: {}", e)
      }
    }
  }
}

#[derive(Debug)]
#[repr(C)]
struct Winsize {
  ws_row: u16,
  ws_col: u16,
  ws_xpixel: u16,
  ws_ypixel: u16
}

impl Winsize {
  const TIOCGWINSZ: u64 = 0x40087468;

  fn get() -> io::Result<(u16, u16)> {
    let wz = Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    let r = unsafe {
      libc::ioctl(libc::STDOUT_FILENO, Self::TIOCGWINSZ, &wz)
    };
 
    match r {
      0 => Ok((wz.ws_col, wz.ws_row)),
      x => Err(io::Error::new(io::ErrorKind::Other, x.to_string()))
    }
  }
}

struct AnsiBuffer {
  buffer: String  
}

impl AnsiBuffer {
  fn new() -> Self {
    AnsiBuffer { buffer: String::new() }
  }

  fn append(&mut self, str: &str) {
    self.buffer.push_str(str)
  }

  fn clear_screen(&mut self) {
    self.buffer.push_str("\x1b[2J")
  }

  fn erase_to_end_of_line(&mut self) {
    self.buffer.push_str("\x1b[K")
  }

  fn move_top_left(&mut self) {
    self.buffer.push_str("\x1b[H")
  }

  fn show_cursor(&mut self) {
    self.buffer.push_str("\x1b[?25h")
  }

  fn hide_cursor(&mut self) {
    self.buffer.push_str("\x1b[?25l")
  }

  fn move_cursor_to(&mut self, row: i32, col: i32) {
    let s = format!("\x1b[{};{}H", row + 1, col + 1);
    self.buffer.push_str(&s)
  }

  fn emit_and_flush(&self, out: &mut io::Stdout) -> io::Result<()> {
    out.write_all(self.buffer.as_bytes())?;
    out.flush()
  }
}

struct Screen {
  stdout: io::Stdout,
  width: u32,
  height: u32,
  cursor: (i32, i32)
}

impl Screen {
  fn new() -> io::Result<Self> {
    let (width, height) = Winsize::get()?;
    let screen = 
      Screen {
        stdout: io::stdout(), 
        width: width as u32,
        height: height as u32,
        cursor: (0, 0)
      };
    Ok(screen)
  }

  fn refresh(&mut self) -> io::Result<()> {
    let mut buffer = AnsiBuffer::new();
    buffer.hide_cursor();
    buffer.move_top_left();
    self.draw_rows(&mut buffer);
    buffer.move_top_left();
    buffer.move_cursor_to(self.cursor.0, self.cursor.1);
    buffer.show_cursor();
    buffer.emit_and_flush(&mut self.stdout)
  }

  fn update_cursor_location(&mut self, row_delta: i32, col_delta: i32) {
    self.cursor.0 += row_delta;
    self.cursor.1 += col_delta;
  }

  fn draw_rows(&mut self, buffer: &mut AnsiBuffer) {
    for i in 1..self.height {
      if i == self.height / 3 {
        let mut blurb = format!("Editeuren editor -- version {}", EDITEUREN_VERSION);
        blurb.truncate(self.width as usize);

        let len = blurb.chars().count() as u32;
        let pad_width = (self.width - len) / 2;
        let padding = String::from_utf8(vec![b' '; pad_width as usize]).unwrap();

        buffer.append(&padding);
        buffer.append(&blurb);
        buffer.erase_to_end_of_line();
        buffer.append("\r\n")
      } else {
        buffer.append("~");
        buffer.erase_to_end_of_line();
        buffer.append("\r\n")
      }
    }
    buffer.erase_to_end_of_line();
    buffer.append("~")
  }
}

struct Editor {
  restore_termios: Termios,
  keyboard: Keyboard,
  screen: Screen,
}

impl Editor {
  fn new() -> io::Result<Self> {
    let original_termios = Editor::enter_raw_mode()?;
    let screen = Screen::new()?;
    let editor =
      Editor {
        restore_termios: original_termios,
        keyboard: Keyboard::new(),
        screen: screen,
      };
    Ok(editor)
  }

  fn restore_console(&mut self) -> io::Result<()> {
    let fd = io::stdin().as_raw_fd();
    tcsetattr(fd, TCSAFLUSH, &self.restore_termios)?;

    let mut buffer = AnsiBuffer::new();
    buffer.clear_screen();
    buffer.move_top_left();
    buffer.emit_and_flush(&mut self.screen.stdout)
  }

  fn enter_raw_mode() -> io::Result<Termios> {
    let fd = io::stdin().as_raw_fd();
    let original_termios = Termios::from_fd(fd)?;

    let mut termios = original_termios.clone();
    termios.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
    termios.c_oflag &= !OPOST;
    termios.c_cflag |= CS8;
    termios.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
    termios.c_cc[VMIN] = 0;
    termios.c_cc[VTIME] = 1;
    tcsetattr(fd, TCSAFLUSH, &termios)?;
  
    Ok(original_termios)
  }

  fn handle_navigation(&mut self, key: char) {
    match key {
      'w' => self.screen.update_cursor_location(-1,  0),
      's' => self.screen.update_cursor_location( 1,  0),
      'a' => self.screen.update_cursor_location( 0, -1),
      'd' => self.screen.update_cursor_location( 0,  1),
      _ => ()
    }
  }

  fn process_key(&mut self) -> bool {
    let key = self.keyboard.read_key();
    self.handle_navigation(key);

    match key {
      c if c == Keyboard::ctrl_key('q') => true,
      c => { print!("{}", c); false }
    }
  }

  fn run_loop(&mut self) -> io::Result<()> {
    loop {
      self.screen.refresh()?;
      if self.process_key() {
        break Ok(())
      }
    }
  }
}

fn main() -> io::Result<()> {
  let mut editor = Editor::new()?;
  editor.run_loop()?;
  editor.restore_console()
}
