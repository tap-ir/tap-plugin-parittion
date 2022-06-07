use std::sync::Arc;

use tap::error::{self};

use crate::{Partition, MBR, SECTOR_SIZE};

use byteorder::{ByteOrder, LittleEndian};

/// Read a DOS/MBR partition table from a 512-byte boot sector, providing a disc sector size.
pub fn mbr_partition_table(sector: &[u8; SECTOR_SIZE]) -> anyhow::Result<Vec<Partition>> 
{
  let mut partitions = Vec::with_capacity(4);

  for entry_id in 0..4 
  {
    let first_entry_offset = 446;
    let entry_size = 16;
    let entry_offset = first_entry_offset + entry_id * entry_size;
    let partition = &sector[entry_offset..entry_offset + entry_size];
    let status = partition[0];
    let bootable = match status 
    {
      0x00 => false,
      0x80 => true,
      _ => 
      {
        return Err(error::RustructError::Unknown(
                   format!("invalid status code in partition {}: {:x}", entry_id, status)).into())
      }
    };

    let type_code = partition[4];
    if type_code == 0 
    {
      continue;
    }

    let start_sector = LittleEndian::read_u32(&partition[8..]);
    let number_of_sector = LittleEndian::read_u32(&partition[12..]); 

    let mbr = MBR{bootable, type_code};

    partitions.push(Partition 
    {
      id: entry_id + 1,
      start_sector : start_sector as u64,
      number_of_sector : number_of_sector as u64,
      mbr : Some(Arc::new(mbr)),
      gpt : None
    });
  }

  Ok(partitions)
}
