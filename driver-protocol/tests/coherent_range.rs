use osaiku_driver_protocol::{direct::*, Error};

#[test]
fn coherent_subrange_rejects_overflow_misalignment_and_other_windows() {
    assert_eq!(CoherentRange::decode(1024,0,0),Ok(None));
    for (pages,offset,length) in [(0,0,0),(1025,0,0),(1024,0,4096),
        (1024,0x1ff000,4096),(1024,0x200001,4096),(1024,0x200000,1),
        (1024,0x200000,0),(1024,0x3ff000,8192),(1024,u64::MAX-4095,4096),
        (1024,SERVICE_COPY_OFFSET,4096),(1024,MMIO_START,4096)] {
        assert_eq!(CoherentRange::decode(pages,offset,length),Err(Error::Range));
    }
    let range=CoherentRange::decode(1024,0x2e2000,0x98000).unwrap().unwrap();
    assert_eq!((range.first_page(),range.pages()),(0x2e2,0x98));
    assert!(range.contains(0x339));
    assert!(!range.contains(0x37a));
    let mut m=Message::new(op::SERVICE_COMPLETE);
    m.offset=0x2e2000;m.length=0x98000;
    assert_eq!(Message::decode(&m.encode()),Ok(m));
}
