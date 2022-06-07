//! Partiton plugin take a VFile attribute and return info and data for each partition 
#![allow(dead_code)]
mod mbr;
mod gpt;

use std::sync::Arc;
use std::io::BufReader;
use std::io::SeekFrom;

use tap::config_schema;
use tap::plugin;
use tap::node::Node;
use tap::plugin::{PluginInfo, PluginInstance, PluginConfig, PluginArgument, PluginResult, PluginEnvironment};
use tap::vfile::{VFile, VFileBuilder};
use tap::mappedvfile::{MappedVFileBuilder,FileRanges};
use tap::tree::{TreeNodeId, TreeNodeIdSchema};
use tap::error::{self, RustructError};
use tap::reflect::{ReflectStruct};
use tap::value::Value;

use serde::{Serialize, Deserialize};
use schemars::{JsonSchema};
use tap_derive::Reflect;

use mbr::mbr_partition_table;

plugin!("partition", "Volume", "Parse MBR & GPT partition", PartitionPlugin, Arguments);

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Arguments
{
  #[schemars(with = "TreeNodeIdSchema")] 
  file : TreeNodeId,
}

#[derive(Debug, Serialize, Deserialize,Default)]
pub struct Results
{
}

#[derive(Default)]
pub struct PartitionPlugin
{
}

impl PartitionPlugin 
{
  fn run(&mut self, args : Arguments, env : PluginEnvironment) -> anyhow::Result<Results>
  {
    let parent_node = env.tree.get_node_from_id(args.file).ok_or(RustructError::ArgumentNotFound("file"))?;
    let value = parent_node.value().get_value("data").ok_or(RustructError::ValueNotFound("data"))?;

    let builder = match value
    {
      Value::VFileBuilder(builder) => builder,
      _ => return Err(error::RustructError::ValueTypeMismatch.into())
    };

    let file = match builder.open()
    {
      Ok(file) => file ,
      Err(err) => { parent_node.value().add_attribute(self.name(), None, None) ; return Err(err) },
    };
    let mut file = BufReader::new(file); 
    let partitions = match Partitions::from_file(&mut file)
    {
       Ok(partitions) => partitions,
       Err(err) => { parent_node.value().add_attribute(self.name(), None, None); return Err(err) },
    };

    //Create tree
    if !partitions.part.is_empty()
    {
      for partition in partitions.part.into_iter()
      {
        let partition_builder = partition.to_builder(builder.clone());
        let partition_name = format!("partition_{}", partition.id);
        let partition_node = Node::new(partition_name);
        partition_node.value().add_attribute("data", partition_builder, None);
        partition_node.value().add_attribute("partition", Arc::new(partition), None);
        env.tree.add_child(args.file, partition_node).unwrap();
      }
    }

    parent_node.value().add_attribute(self.name(), None, None);

    Ok(Results{})
  }
}

/**
 *   Partition parser
 */
const SECTOR_SIZE: usize = 512;

#[derive(Debug)]
pub struct Partitions
{
  pub part : Vec<Partition>
}

impl Partitions 
{
  pub fn from_file<T : VFile>(file : &mut T) -> anyhow::Result<Partitions>
  {
    //check magic
    file.seek(SeekFrom::Start(0))?;
    let mut disc_header = [0u8; 512];
    file.read_exact(&mut disc_header)?;
    if 0x55 != disc_header[510] || 0xAA != disc_header[511] 
    {
       return Err(error::RustructError::Unknown("Partition header not found".into()).into());
    }
    let mbr_part = mbr_partition_table(&disc_header)?;

    match mbr_part.len()
    {
      1 if mbr_part[0].is_gpt() => {},
      _ => return Ok(Partitions{ part: mbr_part}) 
    }
 
    //must found sector size
    let gpt_part = gpt::gpt_from_file(file, 512)?;
    Ok(Partitions{ part: gpt_part})
  } 
}

#[derive(Debug, Reflect)]
pub struct MBR
{
  pub bootable: bool,
  pub type_code: u8,
}

#[derive(Debug, Reflect)]
pub struct GPT
{
  #[reflect(skip)]
  type_uuid: Vec<u8>,//decode to string ?
  #[reflect(skip)]
  partition_uuid: Vec<u8>, //decode to string ?
  #[reflect(skip)]
  attributes: Vec<u8>, //decode it ?
  name: String,
}

fn option_to_value<T>(value : &Option<Arc<T>>) -> Option<Value>
 where T : ReflectStruct + Sync + Send + 'static
{
  value.as_ref().map(|value| Value::ReflectStruct(value.clone()))
}

#[derive(Debug, Reflect)]
pub struct Partition
{
  pub id : usize,
  pub start_sector : u64, //set a sector not size ? 
  pub number_of_sector : u64,  //store a sector not size ?
  #[reflect(with = "option_to_value")]
  pub mbr : Option<Arc<MBR>>, 
  #[reflect(with = "option_to_value")]
  pub gpt : Option<Arc<GPT>>, //We use it this as we don't handle Reflection on enum yet
}

impl Partition
{
  pub fn is_gpt(&self) -> bool 
  {
    const MAXIMUM_SECTOR_SIZE: u32 = 16 * 1024;
    const PROTECTIVE_TYPE: u8 = 0xee;
   
    //rather check number of sector than start_sector ?
    let mbr = match &self.mbr
    {
      Some(mbr) => mbr,
      None => return false, 
    };

    if !mbr.bootable && mbr.type_code == PROTECTIVE_TYPE
    {
       return self.id == 1 && self.start_sector <= MAXIMUM_SECTOR_SIZE  as u64
    }
    false
  }
}

impl Partition
{
  pub fn to_builder(&self, builder : Arc<dyn VFileBuilder>) -> Arc<dyn VFileBuilder>
  {
    let mut file_ranges = FileRanges::new();

    let start = self.start_sector as u64 * 512;
    let len = start + self.number_of_sector as u64 * 512;
    let range = 0 .. len; 
    file_ranges.push(range, start, builder);
    Arc::new(MappedVFileBuilder::new(file_ranges))
  }
}
