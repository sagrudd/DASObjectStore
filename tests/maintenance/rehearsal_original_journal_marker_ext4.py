import array,fcntl,hashlib,json,os,pathlib,signal,subprocess,sys,tempfile
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[2]/'tools'))
import sticky_journal_parent as parent, housekeeping_generation_observer as observer, original_journal_marker as marker
if os.geteuid()!=0: raise SystemExit('requires root and a private mount namespace')
root=pathlib.Path(tempfile.mkdtemp(prefix='das-original-marker-fixture-',dir='/data/mnemosyne-build-output-1000'))
image=root/'ext4.img';fd=os.open(image,os.O_CREAT|os.O_EXCL|os.O_RDWR,0o600);os.ftruncate(fd,64*1024*1024);os.close(fd)
subprocess.run(['/usr/sbin/mkfs.ext4','-q','-F',str(image)],check=True)
m=pathlib.Path(tempfile.mkdtemp(prefix='das-original-marker-',dir='/tmp'));os.chmod(m,0o755);subprocess.run(['/usr/bin/mount','-o','loop,nosuid,nodev',str(image),str(m)],check=True)
results={}
try:
 for point in ('parent_readonly','journal_root','immutable','report_enabled','complete'):
  p=m/('owner-'+point);q=m/('private-'+point);p.mkdir();os.chown(p,994,972);os.chmod(p,0o750);q.mkdir(mode=0o700)
  journal=p/'operation.json';journal.write_bytes(b'old journal');os.chown(journal,994,972);os.chmod(journal,0o600)
  pd=os.open(p,os.O_RDONLY|os.O_DIRECTORY);qd=os.open(q,os.O_RDONLY|os.O_DIRECTORY);ps=os.fstat(pd);js=journal.stat()
  guard=parent.StickyJournalParent(pd,qd,(ps.st_dev,ps.st_ino),(js.st_dev,js.st_ino),hashlib.sha256(b'old journal').hexdigest());guard.acquire()
  obs=observer.ReportGenerationObserver(p);r,w=os.pipe();pid=os.fork()
  if pid==0:
   os.close(r)
   def pause():os.write(w,b'R');signal.pause()
   if point in ('parent_readonly','report_enabled'):
    old=marker.os.fchmod
    def chmod(fd,mode):
     old(fd,mode)
     if mode==(0o1550 if point=='parent_readonly' else 0o1770):pause()
    marker.os.fchmod=chmod
   elif point=='journal_root':
    old=marker.os.fchown
    def chown(fd,uid,gid):old(fd,uid,gid);pause()
    marker.os.fchown=chown
   elif point=='immutable':
    old=marker.fcntl.ioctl
    def ioctl(fd,op,arg,*rest):
     result=old(fd,op,arg,*rest)
     if op==marker._SETFLAGS:pause()
     return result
    marker.fcntl.ioctl=ioctl
   try:
    hold=marker.OriginalJournalMarker(guard,obs);hold.install()
    if point=='complete':
     writer=os.fork()
     if writer==0:
      import time
      os.setgroups([116,122,971,972]);os.setgid(972);os.setuid(994)
      for i in (1,2):
       time.sleep(.4); tmp=p/('latest.tmp.'+str(i));tmp.write_text('report '+str(i));os.replace(tmp,p/'latest.json')
      os._exit(0)
     found=obs.wait_two(5);os.waitpid(writer,0)
     if len(found)!=2:os._exit(5)
     pause()
    os._exit(3)
   except BaseException as e:print('child error',repr(e),flush=True);os._exit(4)
  os.close(w);ready=os.read(r,1);os.close(r)
  if ready!=b'R':raise RuntimeError('child failed before checkpoint '+point)
  os.kill(pid,signal.SIGKILL);os.waitpid(pid,0)
  st=journal.stat();f=os.open(journal,os.O_RDONLY);value=array.array('L',[0]);fcntl.ioctl(f,0x80086601,value,True)
  results[point+'_same_journal_preserved']=(st.st_dev,st.st_ino)==(js.st_dev,js.st_ino) and journal.read_bytes()==b'old journal' and (q/'original-marker-intent.json').is_file()
  results[point+'_ownership_expected']=st.st_uid==(994 if point=='parent_readonly' else 0)
  results[point+'_flag_expected']=bool(value[0]&0x10)==(point in ('immutable','report_enabled','complete'))
  if value[0]&0x10:value[0]&=~0x10;fcntl.ioctl(f,0x40086602,value,True)
  os.close(f);obs.close();guard.close();os.close(pd);os.close(qd)
 results['all_passed']=all(results.values())
finally:
 subprocess.run(['/usr/bin/umount',str(m)],check=True);os.rmdir(m)
 data=(json.dumps(results,sort_keys=True)+'\n').encode();fd=os.open(root/'result.json',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o400);os.write(fd,data);os.fsync(fd);os.close(fd)
print(json.dumps({'root':str(root),'results':results},sort_keys=True))
