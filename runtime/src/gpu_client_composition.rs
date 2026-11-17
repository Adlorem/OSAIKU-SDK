use super::*;
use crate::gpu_compositor::Command as Composition;
use osaiku_gpu_protocol::compositor::{self,Command as Wire};
impl<C:Transport,U:upload::Transport,W:Wait> Client<C,U,W> {
    /// # Safety
    /// Local input spans must be readable, and the statistics output of op 16
    /// writable, throughout this call. No caller pointer is sent to a renderer.
    pub unsafe fn compose(&mut self,c:&mut Composition)->Result<(),isize>{
        if !self.healthy(){return Err(-5);}
        let upload=(2..=5).contains(&c.op)||matches!(c.op,23|24);
        let start=if upload {self.wait.now_ms().ok()} else {None};
        let result=unsafe{self.compose_inner(c)};
        // Local diagnostics only: byte copies, attempted tiles/submissions and
        // elapsed wall time (including seal/register/wait/release). Not wire ABI.
        if upload {c.result[3]=start.zip(self.wait.now_ms().ok()).and_then(|(s,e)|e.checked_sub(s)).unwrap_or(u64::MAX);}
        if result.is_err(){self.failed=true;}result
    }
    fn send_composition(&mut self,commands:&[Wire])->Result<[u64;4],isize>{
        let channel=self.channel.as_ref().ok_or(-5isize)?;let i=channel.identity();
        if i.client as u32!=8{return Err(-8);}
        let len=compositor::encode(channel.next_scope(),(i.width,i.height),commands,&mut self.wire).map_err(|_|-1isize)?;
        self.submit(len)
    }
    unsafe fn compose_inner(&mut self,c:&mut Composition)->Result<(),isize>{
        if c.result!=[0;4]{return Err(-1);}
        if c.op==1{self.connect(c.width,c.height)?;}
        let i=self.channel.as_ref().ok_or(-5isize)?.identity();if i.client as u32!=8{return Err(-8);}
        if c.op==19{
            if c.data.is_null()||c.data as usize%core::mem::align_of::<Composition>()!=0||c.len==0||c.len%core::mem::size_of::<Composition>()!=0||c.len/core::mem::size_of::<Composition>()>32{return Err(-1);}
            let source=unsafe{core::slice::from_raw_parts(c.data.cast::<Composition>(),c.len/core::mem::size_of::<Composition>())};
            let mut commands=[Wire::EMPTY;32];for(n,v)in source.iter().enumerate(){if !crate::gpu_compositor::deferred(v)||v.result!=[0;4]{return Err(-1);}commands[n]=v.wire();}
            c.result=self.send_composition(&commands[..source.len()])?;return Ok(());
        }
        if c.op==16{
            if c.data.is_null()||c.len!=24*8||c.data as usize%8!=0{return Err(-1);}
            let mut snapshot=[0u64;24];
            for n in 0..6{let r=self.send_composition(&[Wire{op:16,args:[n as u64,0,0,0],..Wire::EMPTY}])?;snapshot[n*4..n*4+4].copy_from_slice(&r);}
            unsafe{core::ptr::copy_nonoverlapping(snapshot.as_ptr(),c.data as *mut u64,24);}return Ok(());
        }
        if c.op==23 {
            if !c.data.is_null()||c.len!=0||c.args[0]&crate::window_buffer::TAG==0||c.args[1]==0||c.args[2]>1
                ||c.args[3]<u64::from(c.width)*u64::from(c.height)*4||c.args[3]>crate::window_buffer::MAX_BYTES{return Err(-1);}
            let handle=self.channel.as_mut().unwrap().register(c.args[0],c.args[1],c.args[3])?;
            let command=Wire{op:23,slot:c.slot,width:c.width,height:c.height,rect:c.rect,args:[2,c.args[2],0,0],
                asset:handle,length:u64::from(c.width)*u64::from(c.height)*4,..Wire::EMPTY};
            self.send_composition(&[command])?;
            self.channel.as_mut().unwrap().release(handle)?;
            c.result[..3].copy_from_slice(&[0,0,1]); // no copied tiles, one renderer job
            return Ok(());
        }
        if c.op==24 {
            // Packed background damage, addressed within the retained screen
            // texture. Public renderer wire remains the validated tile opcode.
            let r=c.rect;
            if c.slot!=9||c.data.is_null()||r[2]==0||r[3]==0||c.len!=r[2]as usize*r[3]as usize*4
                ||c.len>upload::MAX_BYTES||c.args!=[0;4]||c.clip!=[0;4]||c.window!=[0;4]{return Err(-1);}
            let mut tile=Wire{op:21,slot:9,width:c.width,height:c.height,rect:r,args:[2,0,0,0],
                ..Wire::EMPTY};
            // Validate geometry before allocating or reading the client span.
            if c.width!=i.width||c.height!=i.height||u64::from(r[0])+u64::from(r[2])>u64::from(c.width)
                ||u64::from(r[1])+u64::from(r[3])>u64::from(c.height){return Err(-1);}
            let source=unsafe{core::slice::from_raw_parts(c.data,c.len)};
            let storage=self.upload.seal(source)?;
            tile.asset=self.channel.as_mut().unwrap().register(storage.id(),storage.generation(),storage.bytes())?;
            tile.length=c.len as u64;c.result[..3].copy_from_slice(&[c.len as u64,1,1]);
            self.send_composition(&[tile])?;
            self.channel.as_mut().unwrap().release(tile.asset)?;
            self.upload.release(storage).map_err(|(e,_)|e)?;
            return Ok(());
        }
        if (2..=5).contains(&c.op){
            if c.width==0||c.height==0||c.width>4160||c.height>4160||c.data.is_null()||c.len!=c.width as usize*c.height as usize*4
                ||c.slot>=12||c.clip!=[0;4]||c.window!=[0;4]||c.args[0]>1||c.args[1..]!=[0;3]{return Err(-1);}
            let kind=if c.op==5{2}else{c.op as u64};
            let init=Wire{op:20,slot:c.slot,width:c.width,height:c.height,args:[kind,c.args[0],0,0],..Wire::EMPTY};
            init.validate((i.width,i.height)).map_err(|_|-1isize)?;
            let rect=if c.op==5{c.rect}else{[0,0,c.width,c.height]};
            if rect[2]==0||rect[3]==0||u64::from(rect[0])+u64::from(rect[2])>u64::from(c.width)||u64::from(rect[1])+u64::from(rect[3])>u64::from(c.height){return Err(-1);}
            if c.op!=5{c.result[2]+=1;self.send_composition(&[init])?;}
            let row=rect[2] as usize*4;let rows=upload::MAX_BYTES/row;let mut y=0usize;
            while y<rect[3] as usize{
                let count=rows.min(rect[3] as usize-y);let len=count*row;
                for r in 0..count{let offset=((rect[1] as usize+y+r)*c.width as usize+rect[0]as usize)*4;
                    let source=unsafe{core::slice::from_raw_parts(c.data.add(offset),row)};self.stage[r*row..(r+1)*row].copy_from_slice(source);}
                let storage=self.upload.seal(&self.stage[..len])?;
                c.result[0]+=len as u64;
                let handle=self.channel.as_mut().unwrap().register(storage.id(),storage.generation(),storage.bytes())?;
                let tile=Wire{op:21,slot:c.slot,width:c.width,height:c.height,rect:[rect[0],rect[1]+y as u32,rect[2],count as u32],args:[kind,0,0,0],asset:handle,length:len as u64,..Wire::EMPTY};
                // An uncertain acknowledgement intentionally retains both
                // kernel owners; teardown, not a timeout, retires the aliases.
                c.result[1]+=1;c.result[2]+=1;self.send_composition(&[tile])?;
                self.channel.as_mut().unwrap().release(handle)?;
                self.upload.release(storage).map_err(|(e,_)|e)?;y+=count;
            }
            if c.op!=5{c.result[2]+=1;self.send_composition(&[Wire{op:22,..init}])?;}
            return Ok(());
        }
        if !c.data.is_null()||c.len!=0{return Err(-1);}
        c.result=self.send_composition(&[c.wire()])?;Ok(())
    }
}
