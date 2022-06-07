//! Get MBR & GPT partition info
extern crate tap_plugin_partition;

use std::env;
use std::fs::File;
use std::sync::Arc;
use std::io::BufReader;

use tap::value::Value;
use tap_plugin_partition::Partitions;

fn main() 
{
   if env::args().len() != 2 
   {
     println!("partition input_file");
     return ;
   }

   let args: Vec<String> = env::args().collect();
   let file_path = &args[1];

   match File::open(file_path)
   {
      Err(_) => println!("Can't open file {}", file_path),
      Ok(file) => 
      {
         let mut buffered = BufReader::new(file);
         let partitions = match Partitions::from_file(&mut buffered)
         {
           Ok(partitions) => partitions,
           Err(err) => {eprintln!("{}", err); return },
         };

         for partition in partitions.part
         {
           let value : Value = Value::ReflectStruct(Arc::new(partition));
           println!("{}", serde_json::to_string(&value).unwrap());
         }
      },
   }
}
