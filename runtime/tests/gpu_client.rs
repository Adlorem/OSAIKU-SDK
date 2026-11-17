use osaiku_runtime::{gpu_client::{Client,Wait},gpu_painter::Command};
use osaiku_gpu_runtime::{channel,upload};
use osaiku_gpu_protocol::{channel::{Message,op,status},buffer::{Handle,kind},render::{Batch,Scope,Draw}};
use osaiku_driver_protocol::shared::{self,Request};
use std::{cell::RefCell,rc::Rc,collections::BTreeMap};
const OWNER:u64=(2<<32)|1001;
const RENDERER:u64=(3<<32)|1002;
#[derive(Default)]
struct State { next:u64,width:u32,height:u32,seq:u64,pages:BTreeMap<u64,u64>,data:BTreeMap<u64,Vec<u8>>,
    seals:Vec<u64>,handles:BTreeMap<u64,u64>,calls:Vec<(char,u64)>,draws:Vec<Draw>,payloads:Vec<Vec<u8>>,
    result:[u64;4],lost:bool,lost_op:u32,max_live:usize,clock:u64,owner:u64,composition:Vec<osaiku_gpu_protocol::compositor::Command> }
fn owner(s:&State)->u64{if s.owner==0{OWNER}else{s.owner}}
#[derive(Clone)]
struct Host(Rc<RefCell<State>>);
fn identity(s:&State)->Message{let mut m=Message::new(op::IDENTITY);m.session=11;m.generation=12;m.status=status::IDLE;
    m.args[..6].copy_from_slice(&[owner(s),RENDERER,9,10,s.width as u64,s.height as u64]);m}
impl channel::Transport for Host {
    fn exchange(&mut self,mut m:Message,bytes:&mut[u8])->Result<Message,isize>{
        m.request().unwrap();let mut s=self.0.borrow_mut();s.calls.push(('c',m.operation as u64));
        match m.operation{
            op::OPEN=>{s.width=m.args[0] as u32;s.height=m.args[1] as u32;s.seq=1;return Ok(identity(&s));}
            op::IDENTITY=>return Ok(identity(&s)),
            op::REGISTER=>{assert!(s.seals.contains(&m.args[0]));s.next+=1;let id=s.next;s.handles.insert(id,m.args[0]);
                m.handle=Handle{owner:owner(&s),device:9,generation:10,serial:id,kind:kind::BUFFER,reserved:0};}
            op::RELEASE=>{assert!(s.handles.remove(&m.handle.serial).is_some());}
            op::RESIZE=>{s.width=m.args[0] as u32;s.height=m.args[1] as u32;}
            op::SUBMIT=>{
                if bytes.starts_with(b"OSAGCB01"){
                    let b=osaiku_gpu_protocol::compositor::Batch::decode(bytes,Scope{session:11,generation:12,sequence:s.seq}).unwrap();
                    for n in 0..b.len(){let c=b.command(n).unwrap();if c.length!=0{let storage=s.handles[&c.asset.serial];assert!(s.seals.contains(&storage));let payload=s.data[&storage][c.offset as usize..(c.offset+c.length)as usize].to_vec();s.payloads.push(payload);}
                        s.result=if c.op==16{core::array::from_fn(|n|c.args[0]*4+n as u64)}else{[0;4]};s.composition.push(c);}
                    if s.lost||s.composition.last().is_some_and(|c|c.op==s.lost_op){return Err(-5);}s.seq+=1;m.status=status::QUEUED;m.length=0;return Ok(m);
                }
                let b=Batch::decode(bytes,Scope{session:11,generation:12,sequence:s.seq}).unwrap();assert_eq!(b.viewport(),(s.width,s.height));
                for n in 0..b.len(){let d=b.draw(n).unwrap().unwrap();if d.length!=0{
                    let storage=s.handles[&d.asset.serial];assert!(s.seals.contains(&storage));let source=&s.data[&storage];
                    let payload=source[d.offset as usize..(d.offset+d.length)as usize].to_vec();s.payloads.push(payload);
                }s.result=if d.operation==2{[(1<<63)|1,(1<<63)|2,u64::from(s.width)*4,0x34325241]}else{[0;4]};s.draws.push(d);}
                if s.lost{return Err(-5);}s.seq+=1;m.status=status::QUEUED;m.length=0;
            }
            op::POLL=>{m.status=status::DONE;m.args[..4].copy_from_slice(&s.result);}
            _=>panic!("unexpected operation {}",m.operation),
        }Ok(m)
    }
}
unsafe impl upload::Transport for Host {
    fn exchange(&mut self,mut m:Request)->Result<Request,isize>{
        let mut s=self.0.borrow_mut();s.calls.push(('u',m.operation));
        match m.operation{
            shared::op::CREATE=>{s.next+=1;m.handle=shared::Handle{id:s.next,generation:1};s.pages.insert(m.handle.id,m.pages);s.max_live=s.max_live.max(s.pages.len());
                assert!(s.pages.len()<=4);assert!(s.pages.values().sum::<u64>()<=1024);}
            shared::op::MAP=>{m.token=m.handle.id;m.address=shared::WINDOW_START;}
            shared::op::SEAL=>{s.seals.push(m.handle.id);m.pages=s.pages[&m.handle.id];m.live=m.pages;m.token=1;}
            shared::op::REVOKE=>{assert!(!s.handles.values().any(|n|*n==m.handle.id));m.returned=s.pages.remove(&m.handle.id).unwrap();s.data.remove(&m.handle.id);s.seals.retain(|n|*n!=m.handle.id);}
            _=>panic!("unexpected upload {}",m.operation),
        }Ok(m)
    }
    fn write(&mut self,address:u64,bytes:&[u8])->Result<(),isize>{assert_eq!(address,shared::WINDOW_START);let mut s=self.0.borrow_mut();let id=s.next;s.data.insert(id,bytes.to_vec());Ok(())}
}
impl Wait for Host{fn now_ms(&self)->Result<u64,isize>{Ok(self.0.borrow().clock)}fn wait(&mut self){self.0.borrow_mut().clock+=1;}}
fn client()->(Box<Client<Host,Host,Host>>,Rc<RefCell<State>>){let s=Rc::new(RefCell::new(State::default()));let h=Host(s.clone());(Box::new(Client::new(h.clone(),h.clone(),h)),s)}
fn begin(c:&mut Client<Host,Host,Host>,width:i32,height:i32){unsafe{c.dispatch(&mut Command{op:1,rect:[0,0,width,height],..Command::EMPTY})}.unwrap();}
fn glyph(data:&[u8])->Command{Command{op:4,rect:[0,0,8,8],clip:[0,0,64,64],params:[255,12,1,0,0,0,0,0],data:data.as_ptr(),len:data.len(),..Command::EMPTY}}
fn draw(c:&mut Client<Host,Host,Host>,commands:&[Command])->Result<(),isize>{unsafe{c.dispatch(&mut Command{op:9,data:commands.as_ptr().cast(),len:core::mem::size_of_val(commands),..Command::EMPTY})}}
#[test]
fn glyphs_share_one_sealed_pack_and_repeated_frames_reuse_exact_asset_identity(){
    let(mut c,s)=client();begin(&mut c,64,64);let data:Vec<_>=(0..32).map(|n|[n;32]).collect();let commands:Vec<_>=data.iter().map(|b|glyph(b)).collect();
    draw(&mut c,&commands).unwrap();let first=s.borrow().draws[1..].to_vec();
    begin(&mut c,64,64);draw(&mut c,&commands).unwrap();let s=s.borrow();
    assert_eq!(s.calls.iter().filter(|m|**m==('u',shared::op::CREATE)).count(),1);
    assert_eq!(s.draws[34..],first);assert_eq!(s.max_live,1);
}
#[test]
fn large_image_tiles_preserve_every_row_without_enlarging_shared_quota(){
    let(mut c,s)=client();begin(&mut c,1024,512);let bytes:Vec<_>=(0..2*1024*1024).map(|n|(n/4096)as u8).collect();
    let image=Command{op:6,rect:[0,0,1024,512],clip:[0,0,1024,512],params:[255,1024,512,0,0,0,0,0],data:bytes.as_ptr(),len:bytes.len(),..Command::EMPTY};
    draw(&mut c,&[image]).unwrap();let s=s.borrow();assert_eq!(s.payloads.concat(),bytes);
    assert_eq!(s.draws[1].rect,[0,0,1024,256]);assert_eq!(s.draws[2].rect,[0,256,1024,256]);assert!(s.pages.values().all(|n|*n<=256));
}
#[test]
fn bad_last_command_is_rejected_before_upload_or_first_draw(){
    let(mut c,s)=client();begin(&mut c,64,64);let data=[1;32];let mut bad=glyph(&data);bad.params[2]=7;let count=s.borrow().calls.len();
    assert!(draw(&mut c,&[glyph(&data),bad]).is_err());assert_eq!(s.borrow().calls.len(),count);
}
#[test]
fn fifth_pack_retires_renderer_alias_before_releasing_shared_pages(){
    let(mut c,s)=client();begin(&mut c,64,64);
    for value in 0..6{draw(&mut c,&[glyph(&[value;32])]).unwrap();}
    let s=s.borrow();assert_eq!(s.max_live,4);assert_eq!(s.calls.iter().filter(|m|**m==('u',shared::op::REVOKE)).count(),2);
    for (n,m) in s.calls.iter().enumerate(){if *m==('u',shared::op::REVOKE){assert_eq!(s.calls[n-1],('c',op::RELEASE as u64));}}
}
#[test]
fn lost_submit_keeps_all_pins_and_prevents_replay(){
    let(mut c,s)=client();begin(&mut c,64,64);s.borrow_mut().lost=true;
    assert!(draw(&mut c,&[glyph(&[3;32])]).is_err());let calls=s.borrow().calls.len();
    assert!(draw(&mut c,&[glyph(&[3;32])]).is_err());let s=s.borrow();assert_eq!(s.calls.len(),calls);assert_eq!(s.pages.len(),1);
    assert!(!s.calls.iter().any(|m|*m==('u',shared::op::REVOKE)));
}
#[test]
fn compositor_uploads_large_texture_in_bounded_sealed_tiles_then_releases_every_alias(){
    use osaiku_runtime::gpu_compositor::Command as C;
    let(mut c,s)=client();s.borrow_mut().owner=(2<<32)|8;
    unsafe{c.compose(&mut C{op:1,width:1920,height:1080,..C::EMPTY})}.unwrap();
    let bytes:Vec<_>=(0..1920*1080*4).map(|n|(n/7680)as u8).collect();
    let mut upload=C{op:2,width:1920,height:1080,data:bytes.as_ptr(),len:bytes.len(),..C::EMPTY};
    unsafe{c.compose(&mut upload)}.unwrap();
    assert_eq!(upload.result[..3],[8_294_400,8,10]);
    let s=s.borrow();assert_eq!(s.payloads.concat(),bytes);assert_eq!(s.max_live,1);assert!(s.pages.is_empty());assert!(s.handles.is_empty());
    assert_eq!(s.composition[1].op,20);assert_eq!(s.composition.last().unwrap().op,22);
    assert!(s.composition.iter().filter(|c|c.op==21).all(|c|c.length<=1024*1024));
}
#[test]
fn compositor_partial_upload_crops_exact_source_rows_and_statistics_use_value_replies(){
    use osaiku_runtime::gpu_compositor::Command as C;
    let(mut c,s)=client();s.borrow_mut().owner=(2<<32)|8;unsafe{c.compose(&mut C{op:1,width:64,height:64,..C::EMPTY})}.unwrap();
    let bytes:Vec<_>=(0..64*64*4).map(|n|n as u8).collect();
    let mut upload=C{op:5,width:64,height:64,rect:[3,4,8,9],data:bytes.as_ptr(),len:bytes.len(),..C::EMPTY};
    unsafe{c.compose(&mut upload)}.unwrap();assert_eq!(upload.result[..3],[288,1,1]);
    let expected:Vec<_>=(4..13).flat_map(|y|bytes[(y*64+3)*4..(y*64+11)*4].iter().copied()).collect();assert_eq!(s.borrow().payloads.concat(),expected);
    let mut out=[u64::MAX;24];unsafe{c.compose(&mut C{op:16,data:out.as_mut_ptr().cast(),len:192,..C::EMPTY})}.unwrap();assert_eq!(out,core::array::from_fn(|n|n as u64));
}
#[test]
fn ordinary_painter_client_cannot_submit_display_commands(){
    use osaiku_runtime::gpu_compositor::Command as C;
    let(mut c,s)=client();assert!(unsafe{c.compose(&mut C{op:1,width:64,height:64,..C::EMPTY})}.is_err());assert!(s.borrow().composition.is_empty());
}
#[test]
fn retained_background_damage_sends_one_packed_tile_and_checks_its_bounds(){
    use osaiku_runtime::gpu_compositor::Command as C;
    for invalid in [false,true]{
        let(mut c,s)=client();s.borrow_mut().owner=(2<<32)|8;
        unsafe{c.compose(&mut C{op:1,width:640,height:480,..C::EMPTY})}.unwrap();
        let bytes=vec![37;72*24*4];
        let mut tile=C{op:24,slot:9,width:640,height:480,rect:[if invalid{639}else{100},420,72,24],
            data:bytes.as_ptr(),len:bytes.len(),..C::EMPTY};
        let result=unsafe{c.compose(&mut tile)};
        if invalid {assert!(result.is_err());assert!(s.borrow().payloads.is_empty());}
        else {
            result.unwrap();assert_eq!(tile.result[..3],[6912,1,1]);
            let s=s.borrow();assert_eq!(s.payloads,[bytes]);assert_eq!(s.composition.last().unwrap().rect,[100,420,72,24]);
            assert!(s.handles.is_empty());assert!(s.pages.is_empty());
        }
    }
}

#[test]
fn uncertain_compositor_tile_keeps_aliases_and_reports_partial_progress(){
    use osaiku_runtime::gpu_compositor::Command as C;
    let(mut c,s)=client();s.borrow_mut().owner=(2<<32)|8;
    unsafe{c.compose(&mut C{op:1,width:1024,height:512,..C::EMPTY})}.unwrap();
    s.borrow_mut().lost_op=21;
    let bytes=vec![7;1024*512*4];
    let mut upload=C{op:2,width:1024,height:512,data:bytes.as_ptr(),len:bytes.len(),..C::EMPTY};
    assert!(unsafe{c.compose(&mut upload)}.is_err());
    assert_eq!(upload.result[..3],[1024*1024,1,2]);
    assert_eq!(s.borrow().pages.len(),1);assert_eq!(s.borrow().handles.len(),1);
    let before=s.borrow().calls.len();
    assert!(unsafe{c.compose(&mut C{op:2,width:1024,height:512,data:bytes.as_ptr(),len:bytes.len(),..C::EMPTY})}.is_err());
    assert_eq!(s.borrow().calls.len(),before,"uncertain upload cannot be replayed");
}

#[test]
fn retained_full_hd_window_uses_one_job_and_no_tile_allocations(){
    use osaiku_runtime::{gpu_compositor::Command as C,window_buffer::TAG};
    let(mut c,s)=client();s.borrow_mut().owner=(2<<32)|8;
    unsafe{c.compose(&mut C{op:1,width:1920,height:1080,..C::EMPTY})}.unwrap();
    let id=TAG|42;let bytes=vec![31;1920*1080*4];
    {let mut s=s.borrow_mut();s.seals.push(id);s.data.insert(id,bytes.clone());}
    for (generation,rect,new) in [(1,[0,0,1920,1080],1),(2,[9,16,75,40],0)]{
        let mut command=C{op:23,width:1920,height:1080,rect,args:[id,generation,new,bytes.len() as u64],..C::EMPTY};
        unsafe{c.compose(&mut command)}.unwrap();
        assert_eq!(command.result[..3],[0,0,1]);
    }
    let s=s.borrow();assert_eq!(s.composition.iter().filter(|c|c.op==23).count(),2);
    assert!(s.calls.iter().all(|(kind,_)|*kind!='u'),"no CREATE/MAP/SEAL/REVOKE tile traffic");
    assert!(s.handles.is_empty());assert_eq!(s.payloads,[bytes.clone(),bytes]);
    assert_eq!(s.composition[2].rect,[9,16,75,40]);
}

#[test]
fn lost_shared_upload_keeps_its_reader_lease_and_cannot_replay(){
    use osaiku_runtime::{gpu_compositor::Command as C,window_buffer::TAG};
    let(mut c,s)=client();s.borrow_mut().owner=(2<<32)|8;
    unsafe{c.compose(&mut C{op:1,width:64,height:64,..C::EMPTY})}.unwrap();
    let id=TAG|43;{let mut s=s.borrow_mut();s.seals.push(id);s.data.insert(id,vec![0;16384]);s.lost_op=23;}
    let mut command=C{op:23,width:64,height:64,rect:[0,0,64,64],args:[id,1,1,16384],..C::EMPTY};
    assert!(unsafe{c.compose(&mut command)}.is_err());
    assert_eq!(s.borrow().handles.len(),1);
    let before=s.borrow().calls.len();assert!(unsafe{c.compose(&mut command)}.is_err());assert_eq!(before,s.borrow().calls.len());
}

#[test]
fn shared_window_wire_rejects_partial_creation_and_invalid_extents(){
    use osaiku_gpu_protocol::{buffer::{Handle,kind},compositor::{self,Command},render::Scope};
    let base=Command{op:23,slot:0,width:1920,height:1080,rect:[0,0,1920,1080],args:[2,1,0,0],
        asset:Handle{owner:(2<<32)|8,device:1,generation:1,serial:1,kind:kind::BUFFER,reserved:0},
        length:1920*1080*4,..Command::EMPTY};
    assert!(base.validate((1920,1080)).is_ok());
    assert!(Command{args:[2,0,0,0],rect:[4,8,32,16],..base}.validate((1920,1080)).is_ok());
    for invalid in [
        Command{rect:[4,8,32,16],..base}, // A new texture needs all initial pixels.
        Command{offset:4096,..base},
        Command{length:base.length-4,..base},
        Command{width:4160,height:4160,length:4160*4160*4,rect:[0,0,4160,4160],..base},
        Command{slot:8,..base},
        Command{args:[2,0,0,0],rect:[1919,0,2,1],..base},
        Command{args:[2,0,0,1],..base},
    ] {assert!(invalid.validate((1920,1080)).is_err());}
    let mut wire=[0;1024];
    assert!(compositor::encode(Scope{session:1,generation:1,sequence:1},(1920,1080),&[base,base],&mut wire).is_err());
}
