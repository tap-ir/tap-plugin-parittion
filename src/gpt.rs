use std::sync::Arc;
use std::io::SeekFrom;

use tap::vfile::VFile;
use tap::error;

use crate::{Partition, GPT};

use byteorder::{ByteOrder, LittleEndian};
use crc::crc32::checksum_ieee;

pub fn gpt_from_file<T: VFile>(file: &mut T, sector_size: u64) -> anyhow::Result<Vec<Partition>>
{
    file.seek(SeekFrom::Start(sector_size))?;

    let mut lba1 = vec![0u8; sector_size as usize];
    file.read_exact(&mut lba1)?;

    if b"EFI PART" != &lba1[0x00..0x08] 
    {
      return Err(error::RustructError::Unknown("bad EFI signature".into()).into());
    }

    if [0, 0, 1, 0] != lba1[0x08..0x0c] 
    {
      return Err(error::RustructError::Unknown("unsupported revision".into()).into());
    }
    let header_size = LittleEndian::read_u32(&lba1[0x0c..0x10]);
    if header_size < 92 
    {
      return Err(error::RustructError::Unknown("header too short".into()).into());
    }

    let header_crc = LittleEndian::read_u32(&lba1[0x10..0x14]);

    #[allow(clippy::needless_range_loop)]
    for crc_part in 0x10..0x14 
    {
        lba1[crc_part] = 0;
    }

    if header_crc != checksum_ieee(&lba1[..header_size as usize]) 
    {
        return Err(error::RustructError::Unknown("header checksum mismatch".into()).into());
    }

    if 0 != LittleEndian::read_u32(&lba1[0x14..0x18]) 
    {
        return Err(error::RustructError::Unknown("unsupported data in reserved field 0x0c".into()).into());
    }

    if 1 != LittleEndian::read_u64(&lba1[0x18..0x20]) 
    {
        return Err(error::RustructError::Unknown("current lba must be '1' for first header".into()).into());
    }

    let first_usable_lba = LittleEndian::read_u64(&lba1[0x28..0x30]);
    let last_usable_lba = LittleEndian::read_u64(&lba1[0x30..0x38]);

    if first_usable_lba > last_usable_lba 
    {
        return Err(error::RustructError::Unknown("usable lbas are backwards?!".into()).into());
    }

    let mut guid = [0u8; 16];
    guid.copy_from_slice(&lba1[0x38..0x48]);

    if 2 != LittleEndian::read_u64(&lba1[0x48..0x50]) 
    {
        return Err(error::RustructError::Unknown("starting lba must be '2' for first header".into()).into());
    }

    let entries = LittleEndian::read_u32(&lba1[0x50..0x54]);
    let entry_size = LittleEndian::read_u32(&lba1[0x54..0x58]);

    if entry_size < 128 
    {
        return Err(error::RustructError::Unknown("entry size is implausibly small".into()).into());
    }


    if first_usable_lba < 2 + ((u64::from(entry_size) * u64::from(entries)) / sector_size) 
    {
        return Err(error::RustructError::Unknown("first usable lba is too low".into()).into());
    }

    if !all_zero(&lba1[header_size as usize..]) 
    {
        return Err(error::RustructError::Unknown("reserved header tail is not all empty".into()).into());
    }

    let mut table = vec![0u8; entry_size as usize  * entries as usize];
    file.read_exact(&mut table)?;

    let mut ret = Vec::with_capacity(16);
    for id in 0..entries as usize
    {
      let entry_size = entry_size as usize;
      let entry = &table[id * entry_size..(id + 1) * entry_size];
      let type_uuid = &entry[0x00..0x10];
      if all_zero(type_uuid) 
      {
        continue;
      }

      let partition_uuid = &entry[0x10..0x20];
      let first_lba = LittleEndian::read_u64(&entry[0x20..0x28]);
      let last_lba = LittleEndian::read_u64(&entry[0x28..0x30]);

      if first_lba > last_lba || first_lba < first_usable_lba || last_lba > last_usable_lba 
      {
        return Err(error::RustructError::Unknown("partition entry is out of range".into()).into());
      }

      let attributes = &entry[0x30..0x38];
      let name_data = &entry[0x38..0x80];
      let name_le: Vec<u16> = (0..(0x80 - 0x38) / 2)
            .map(|idx| LittleEndian::read_u16(&name_data[2 * idx..2 * (idx + 1)]))
            .take_while(|val| 0 != *val)
            .collect();

      let name = match String::from_utf16(&name_le) 
      {
        Ok(name) => name,
        Err(e) =>  return Err(error::RustructError::Unknown(format!("partition {} has an invalid name: {:?}", id, e)).into()),
      };

      let gpt = GPT{type_uuid : type_uuid.to_vec(), partition_uuid : partition_uuid.to_vec(), 
                    attributes : attributes.to_vec(), name };

      ret.push(Partition
      {
        id : id + 1,
        start_sector : first_lba,
        number_of_sector : (last_lba - first_lba + 1),
        mbr : None,
        gpt : Some(Arc::new(gpt)),
      });
    }

    Ok(ret)
}

fn all_zero(val: &[u8]) -> bool 
{
  val.iter().all(|x| 0 == *x)
}
