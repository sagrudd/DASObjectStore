import array,fcntl,hashlib,importlib.util,json,os,pathlib,signal,subprocess,sys,tempfile
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[2]/'tools'))
import sticky_journal_parent as parent, housekeeping_generation_observer as observer, journal_marker_hold as marker
if os.geteuid()!=0: raise SystemExit('requires root and a private mount namespace')
root=pathlib.Path(tempfile.mkdtemp(prefix='das-marker-hold-fixture-',dir='/data/mnemosyne-build-output-1000'))
image=root/'ext4.img';fd=os.open(image,os.O_CREAT|os.O_EXCL|os.O_RDWR,0o600);os.ftruncate(fd,64*1024*1024);os.close(fd)
subprocess.run(['/usr/sbin/mkfs.ext4','-q','-F',str(image)],check=True)
m=pathlib.Path(tempfile.mkdtemp(prefix='das-marker-crash-',dir='/tmp'));os.chmod(m,0o755);subprocess.run(['/usr/bin/mount','-o','loop,nosuid,nodev',str(image),str(m)],check=True)
results={}
try:
 for point in ('before_flag','after_flag','complete'):
  p=m/('owner-'+point);q=m/('private-'+point);p.mkdir();os.chown(p,994,972);os.chmod(p,0o750);q.mkdir(mode=0o700)
  journal=p/'operation.json';journal.write_bytes(b'old journal');os.chown(journal,994,972);os.chmod(journal,0o600)
  pd=os.open(p,os.O_RDONLY|os.O_DIRECTORY);qd=os.open(q,os.O_RDONLY|os.O_DIRECTORY);ps=os.fstat(pd);js=journal.stat()
  guard=parent.StickyJournalParent(pd,qd,(ps.st_dev,ps.st_ino),(js.st_dev,js.st_ino),hashlib.sha256(b'old journal').hexdigest());guard.acquire()
  obs=observer.ReportGenerationObserver(p);r,w=os.pipe();pid=os.fork()
  if pid==0:
   os.close(r)
   def pause():os.write(w,b'R');signal.pause()
   old=marker._set_immutable
   if point=='before_flag':
    def setting(fd):pause();old(fd)
    marker._set_immutable=setting
   elif point=='after_flag':
    def setting(fd):old(fd);pause()
    marker._set_immutable=setting
   try:
    hold=marker.JournalMarkerHold(guard,qd,obs);hold.install()
    if point=='complete':pause()
    os._exit(3)
   except BaseException as e:print('child error',repr(e),flush=True);os._exit(4)
  os.close(w);ready=os.read(r,1);os.close(r)
  if ready!=b'R':raise RuntimeError('child failed before checkpoint '+point)
  os.kill(pid,signal.SIGKILL);os.waitpid(pid,0)
  marker_path=p/'operation.json'; preserved=q/'marker';s=marker_path.stat();f=os.open(marker_path,os.O_RDONLY);value=array.array('L',[0]);fcntl.ioctl(f,0x80086601,value,True)
  results[point+'_marker_and_old_preserved']=(s.st_uid==0 and s.st_gid==0 and (s.st_mode&0o777)==0o600 and preserved.read_bytes()==b'old journal' and (q/'marker-intent.json').is_file())
  results[point+'_expected_flag']=bool(value[0]&0x10)==(point!='before_flag')
  child=os.fork()
  if child==0:
   os.setgroups([116,122,971,972]);os.setgid(972);os.setuid(994)
   try:os.unlink(marker_path);os._exit(2)
   except PermissionError:os._exit(0)
  _,status=os.waitpid(child,0);results[point+'_owner_cannot_unlink']=os.waitstatus_to_exitcode(status)==0
  if value[0]&0x10:value[0]&=~0x10;fcntl.ioctl(f,0x40086602,value,True)
  os.close(f);obs.close();guard.close();os.close(pd);os.close(qd)
 results['all_passed']=all(results.values())
finally:
 subprocess.run(['/usr/bin/umount',str(m)],check=True);os.rmdir(m)
 data=(json.dumps(results,sort_keys=True)+'\n').encode();fd=os.open(root/'result.json',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o400);os.write(fd,data);os.fsync(fd);os.close(fd)
print(json.dumps({'root':str(root),'results':results},sort_keys=True))
