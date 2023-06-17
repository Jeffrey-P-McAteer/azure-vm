
#[allow(unused_macros)]
#[macro_export]
macro_rules! dump_error {
  ($e:expr) => {
    if let Err(err) = $e {
      eprintln!("ERROR {}:{}> {:?}", err, file!(), line!());
    }
  }
}

#[allow(unused_macros)]
#[macro_export]
macro_rules! dump_error_async {
  ($e:expr) => {
    async {
      if let Err(err) = $e.await {
        eprintln!("ERROR {}:{}> {:?}", err, file!(), line!());
      }
    }
  }
}

#[allow(unused_macros)]
#[macro_export]
macro_rules! dump_error_and_ret {
  ($e:expr) => {
    match $e {
      Err(err) => {
        eprintln!("ERROR {}:{}> {:?}", err, file!(), line!());
        return;
      }
      Ok(val) => val
    }
  }
}
