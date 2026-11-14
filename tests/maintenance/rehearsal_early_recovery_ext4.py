#!/usr/bin/env python3
"""Root-only disposable four-ext4 checkpoint rehearsal; never targets a NUC."""
import array,fcntl,hashlib,json,os,pathlib,signal,sqlite3,stat,subprocess,sys,tempfile,time
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[2]/'tools'))
import sticky_journal_parent as parent,housekeeping_generation_observer as observer,original_journal_marker as marker,integrated_claim_recovery as recovery,recovered_journal_marker as recovered
if os.geteuid()!=0:raise SystemExit('requires root and private mount namespace')
base=pathlib.Path('/data/mnemosyne-build-output-1000');root=pathlib.Path(tempfile.mkdtemp(prefix='das-early-fixture-',dir=base))
mounts=[];results={};details={};jroot=None
try:
 for name in ('journal','source','destination','backup'):
  image=root/(name+'.ext4');fd=os.open(image,os.O_CREAT|os.O_EXCL|os.O_RDWR,0o600);os.ftruncate(fd,64*1024*1024);os.close(fd)
  subprocess.run(['/usr/sbin/mkfs.ext4','-q','-F',str(image)],check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
  mount=pathlib.Path(tempfile.mkdtemp(prefix='das-early-'+name+'-',dir='/tmp'));os.chmod(mount,0o755)
  subprocess.run(['/usr/bin/mount','-o','loop,nosuid,nodev',str(image),str(mount)],check=True);mounts.append(mount)
 jm,sm,dm,bm=mounts
 phases=('prepared','before-source-parent','source-parent-held','before-partial-parent','partial-parent-held','before-source-chown','before-source-immutable','source-immutable','before-partial-chown','before-partial-immutable','partial-immutable','before-quarantine','after-rename-before-fsync','quarantined','before-commit','committed','deny-incomplete','deny-tamper','deny-acl','deny-quarantine-conflict','deny-out-of-order','after-first-parent-chmod','after-partial-file-chown','after-partial-immutable-clear','after-db-commit-before-state','during-state-rename','deny-committed-regressed','retry-kill-after-partial-immutable','retry-kill-committed-validation')
 selected=os.environ.get('DAS_EARLY_CASES')
 if selected:
  requested=selected.split(',')
  if any(x not in phases for x in requested) or len(set(requested))!=len(requested):raise RuntimeError('invalid case selection')
  phases=tuple(requested)
 for i,phase in enumerate(phases):
  case=str(i);jr=jm/case;jr.mkdir();jdir=jr/'journal';jdir.mkdir();os.chown(jdir,994,972);os.chmod(jdir,0o750)
  state=jr/'state';state.mkdir(mode=0o700)
  sp=sm/case;sp.mkdir();os.chown(sp,994,972);os.chmod(sp,0o750)
  dp=dm/case;dp.mkdir();os.chown(dp,994,972);os.chmod(dp,0o770)
  for directory,mask in ((sp,'r-x'),(dp,'rwx')):
   subprocess.run(['/usr/bin/setfacl','-m','g:1001:rwx,m::'+mask+',d:u::rwx,d:g::r-x,d:g:1001:rwx,d:m::rwx,d:o::---',str(directory)],check=True)
  q=dm/('q'+case);q.mkdir(mode=0o700);backup=bm/case;backup.mkdir(mode=0o700)
  sf=sp/'payload';df=dp/'payload';sf.write_bytes(b'complete');df.write_bytes(b'partial');os.chown(sf,994,972);os.chmod(sf,0o640);os.chown(df,994,972);os.chmod(df,0o660)
  ss,ds=sf.stat(),df.stat();ssha=hashlib.sha256(b'complete').hexdigest();dsha=hashlib.sha256(b'partial').hexdigest()
  candidate={'placement_id':'placement','object_id':'object','store_id':'store','disk_id':'source-disk','disk_state':'Watch','relative_path':'objects/payload','size_bytes':8,'content_hash':ssha,'verified_at_utc':'2026-01-01T00:00:00Z','existing_disk_ids':['source-disk','other-disk']}
  claim=dict(zip(('claim_id','claim_kind','owner_id','request_id','request_digest','disk_id','reserved_bytes','consumed_bytes','created_at_utc'),('claim-1','repair','owner','request','digest','disk',8,0,'2026-01-01T00:00:00Z')))
  intent={'claim':claim,'candidate':candidate,'source_sha256':ssha,'original_updated_at':'2026-01-01T00:00:00Z','original_lease':[None,None],'release_time':'2026-01-02T00:00:00Z'}
  journal=jdir/'operation.json';journal.write_text(json.dumps({'schema':'dasobjectstore.storage_assurance.operation.v1','operation_id':'fixture','action':'rotate','phase':'claimed','candidate':candidate,'destination_disk_id':'disk','destination_relative_path':'objects/payload','claim_owner':'owner','created_at_utc':'t','updated_at_utc':'t'}));os.chown(journal,994,972);os.chmod(journal,0o600)
  rows=[{'name':'source','size':8,'sha256':ssha,'device':ss.st_dev,'inode':ss.st_ino},{'name':'partial','size':7,'sha256':dsha,'device':ds.st_dev,'inode':ds.st_ino}]
  for name,data in (('source',b'complete'),('partial',b'partial')):(backup/name).write_bytes(data);os.chmod(backup/name,0o600)
  receipt=backup/'receipt.json';receipt.write_text(json.dumps(rows));os.chmod(receipt,0o600)
  recovery.EXPECTED_BACKUP_RECEIPT_SHA256=hashlib.sha256(receipt.read_bytes()).hexdigest()
  journal_sha=hashlib.sha256(journal.read_bytes()).hexdigest();recovery.EXPECTED_ORIGINAL_JOURNAL_SHA256=journal_sha;recovered._EXPECTED_JOURNAL=journal_sha
  dbpath=jr/'live.sqlite';db=sqlite3.connect(dbpath,isolation_level=None)
  schema=(pathlib.Path(__file__).resolve().parents[2]/'crates/dasobjectstore-metadata/src/schema.rs').read_text().split('pub const LIVE_SCHEMA_SQL: &str = r#"',1)[1].split('"#;',1)[0]
  db.executescript(schema);db.execute('PRAGMA foreign_keys=OFF')
  db.execute("INSERT INTO objects(object_id,store_id,state,size_bytes,content_hash,created_at_utc,updated_at_utc) VALUES('object','store','HddCopyVerified',8,?,'t','t')",(ssha,))
  db.execute("INSERT INTO placements(placement_id,object_id,disk_id,relative_path,content_hash,created_at_utc) VALUES('placement','object','source-disk','objects/payload',?,'t')",(ssha,))
  db.execute("INSERT INTO disk_capacity_claims(claim_id,claim_kind,owner_id,request_id,request_digest,disk_id,reserved_bytes,consumed_bytes,created_at_utc,state,released_at_utc,updated_at_utc,lease_owner,lease_expires_at_utc) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",tuple(claim[k] for k in ('claim_id','claim_kind','owner_id','request_id','request_digest','disk_id','reserved_bytes','consumed_bytes','created_at_utc'))+('active',None,'2026-01-01T00:00:00Z',None,None));db.close()
  bfd=os.open(backup,os.O_RDONLY|os.O_DIRECTORY);jfd=os.open(jdir,os.O_RDONLY|os.O_DIRECTORY);statefd=os.open(state,os.O_RDONLY|os.O_DIRECTORY);qfd=os.open(q,os.O_RDONLY|os.O_DIRECTORY);spfd=os.open(sp,os.O_RDONLY|os.O_DIRECTORY);dpfd=os.open(dp,os.O_RDONLY|os.O_DIRECTORY);sfd=os.open(sf,os.O_RDONLY);dfd=os.open(df,os.O_RDONLY)
  fds=(bfd,jfd,statefd,qfd,spfd,dpfd,sfd,dfd)
  intent['filesystem']={}
  for name,dirfd,filefd in (('source',spfd,sfd),('partial',dpfd,dfd)):
   d=os.fstat(dirfd);f=os.fstat(filefd);intent['filesystem'][name]={'parent_device':d.st_dev,'parent_inode':d.st_ino,'parent_uid':d.st_uid,'parent_gid':d.st_gid,'parent_mode':stat.S_IMODE(d.st_mode),'parent_acl':parent._acl(dirfd),'file_uid':f.st_uid,'file_gid':f.st_gid,'file_mode':stat.S_IMODE(f.st_mode),'file_acl':parent._acl(filefd),'file_flags':marker._flags(filefd)}
  pid=os.fork()
  if pid==0:
   try:
    backups=recovery.VerifiedBackups(bfd);ps=os.fstat(jfd);js=journal.stat();guard=parent.StickyJournalParent(jfd,statefd,(ps.st_dev,ps.st_ino),(js.st_dev,js.st_ino),journal_sha);guard.acquire();obs=observer.ReportGenerationObserver(jdir);held=marker.OriginalJournalMarker(guard,obs);held.install()
    source=recovery.Payload(spfd,sfd,backups.rows['source']);partial=recovery.Payload(dpfd,dfd,backups.rows['partial']);conn=sqlite3.connect(dbpath,isolation_level=None)
    writer=os.fork()
    if writer==0:
     os.setgroups([116,122,971,972]);os.setgid(972);os.setuid(994)
     for k in (1,2):time.sleep(.2);tmp=jdir/('report-'+str(k));tmp.write_text('report '+str(k));os.replace(tmp,jdir/'latest.json')
     os._exit(0)
    coord=recovery.IntegratedClaimRecovery(held,backups,source,partial,statefd,qfd,conn,intent);original=coord._write;reaped=[False]
    def checkpoint(value):
     if not reaped[0]:os.waitpid(writer,0);reaped[0]=True
     if phase=='after-db-commit-before-state' and value=='committed':os.kill(os.getpid(),signal.SIGKILL)
     original(value)
     if value==('before-partial-chown' if phase=='retry-kill-after-partial-immutable' else 'committed' if phase in ('deny-committed-regressed','retry-kill-committed-validation') else 'prepared' if phase.startswith('deny-') else phase):os.kill(os.getpid(),signal.SIGKILL)
    coord._write=checkpoint
    if phase=='after-first-parent-chmod':
     native_chmod=os.fchmod
     def kill_chmod(fd,mode):
      native_chmod(fd,mode)
      if fd==coord.source.directory_fd and mode==0o500:os.kill(os.getpid(),signal.SIGKILL)
     os.fchmod=kill_chmod
    if phase=='after-partial-file-chown':
     native_chown=os.fchown
     def kill_chown(fd,uid,gid):
      native_chown(fd,uid,gid)
      if fd==coord.partial.file_fd and (uid,gid)==(0,0):os.kill(os.getpid(),signal.SIGKILL)
     os.fchown=kill_chown
    if phase=='after-partial-immutable-clear':
     native_ioctl=fcntl.ioctl
     def kill_ioctl(fd,operation,arg,*extra):
      result=native_ioctl(fd,operation,arg,*extra)
      if fd==coord.partial.file_fd and operation==recovery._SETFLAGS and not (arg[0]&recovery._IMMUTABLE):os.kill(os.getpid(),signal.SIGKILL)
      return result
     fcntl.ioctl=kill_ioctl
    if phase=='during-state-rename':
     native_rename=os.rename
     def kill_state_rename(src,dst,*args,**kwargs):
      if src=='recovery.new' and coord.record['phase']=='before-partial-parent':os.kill(os.getpid(),signal.SIGKILL)
      return native_rename(src,dst,*args,**kwargs)
     os.rename=kill_state_rename
    if phase=='after-rename-before-fsync':
     native=recovery._libc.renameat2
     def kill_after(*args):
      result=native(*args)
      if result==0:os.kill(os.getpid(),signal.SIGKILL)
      return result
     recovery._libc.renameat2=kill_after
    coord.run(timeout=20);os._exit(9)
   except BaseException as exc:
    print('case child error',phase,repr(exc),flush=True);os._exit(8)
  _,status=os.waitpid(pid,0)
  if not os.WIFSIGNALED(status) or os.WTERMSIG(status)!=signal.SIGKILL:raise RuntimeError('checkpoint not killed: '+phase)
  details[phase]={'initial_signal':signal.SIGKILL,'retry_signal':None}
  if phase=='deny-incomplete':(state/'recovery.new').write_bytes(b'partial')
  elif phase=='deny-tamper':df.write_bytes(b'corrupt')
  elif phase=='deny-acl':os.chmod(dp,0o777)
  elif phase=='deny-quarantine-conflict':(q/'partial').write_bytes(b'foreign')
  elif phase=='deny-committed-regressed':
   db=sqlite3.connect(dbpath)
   db.execute("UPDATE disk_capacity_claims SET state='active',released_at_utc=NULL,updated_at_utc='2026-01-01T00:00:00Z' WHERE claim_id='claim-1'");db.commit();db.close()
  elif phase=='deny-out-of-order':
   path=state/'recovery.json';data=json.loads(path.read_bytes());data['phase']='partial-immutable';path.write_text(json.dumps(data))
  backups=recovery.VerifiedBackups(bfd);held=recovered.RecoveredJournalMarker(jdir,statefd)
  if phase in ('retry-kill-after-partial-immutable','retry-kill-committed-validation'):
   retry=os.fork()
   if retry==0:
    conn=sqlite3.connect(dbpath,isolation_level=None);old_write=recovery._write_recovery_record
    def kill_retry(fd,data):
     old_write(fd,data)
     if data['phase']=='partial-immutable':os.kill(os.getpid(),signal.SIGKILL)
    recovery._write_recovery_record=kill_retry
    if phase=='retry-kill-committed-validation':
     def kill_validation(*args,**kwargs):os.kill(os.getpid(),signal.SIGKILL)
     recovery.validate_exact_claim=kill_validation
    try:recovery.resume_early(held,backups,spfd,dpfd,qfd,statefd,conn);os._exit(9)
    except BaseException as exc:print('retry child error',repr(exc),flush=True);os._exit(8)
   _,retry_status=os.waitpid(retry,0)
   if not os.WIFSIGNALED(retry_status) or os.WTERMSIG(retry_status)!=signal.SIGKILL:raise RuntimeError('retry checkpoint not killed')
   details[phase]['retry_signal']=signal.SIGKILL
   if phase=='retry-kill-committed-validation' and json.loads((state/'recovery.json').read_text())['phase']!='committed':raise RuntimeError('committed phase regressed')
  conn=sqlite3.connect(dbpath,isolation_level=None)
  try:
   if phase.startswith('deny-') or phase=='during-state-rename':
    denied=False
    try:recovery.resume_early(held,backups,spfd,dpfd,qfd,statefd,conn)
    except (RuntimeError,ValueError,OSError):denied=True
    row=conn.execute("SELECT state FROM disk_capacity_claims WHERE claim_id='claim-1'").fetchone()
    results[phase]=(denied and row==('active',) and journal.is_file() and bool(marker._flags(held.journal_fd)&0x10) and (phase!='deny-committed-regressed' or json.loads((state/'recovery.json').read_text())['phase']=='committed'))
   else:
    value=recovery.resume_early(held,backups,spfd,dpfd,qfd,statefd,conn)
    row=conn.execute("SELECT state FROM disk_capacity_claims WHERE claim_id='claim-1'").fetchone()
    results[phase]=(value=='committed-marker-retained' and row==('released',) and journal.is_file() and sf.read_bytes()==b'complete' and (q/'partial').read_bytes()==b'partial' and not df.exists() and ss.st_dev!=ds.st_dev and os.stat(backup).st_dev not in (ss.st_dev,ds.st_dev))
  finally:conn.close();held.close();backups.close()
  for path in (sf,journal):
   f=os.open(path,os.O_RDONLY);v=array.array('L',[0]);fcntl.ioctl(f,0x80086601,v,True);v[0]&=~0x10;fcntl.ioctl(f,0x40086602,v,True);os.close(f)
  for fd in fds:os.close(fd)
  details[phase]['result']='denied' if phase.startswith('deny-') or phase=='during-state-rename' else 'committed-marker-retained'
  details[phase]['passed']=results[phase]
  if not results[phase]:raise RuntimeError('recovery assertion failed: '+phase)
 results['all_passed']=all(results.values())
finally:
 for mount in reversed(mounts):subprocess.run(['/usr/bin/umount',str(mount)],check=True);os.rmdir(mount)
 data=(json.dumps({'results':results,'details':details},sort_keys=True)+'\n').encode();fd=os.open(root/'result.json',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o400);os.write(fd,data);os.fsync(fd);os.close(fd)
print(json.dumps({'root':str(root),'results':results},sort_keys=True));sys.exit(0 if results.get('all_passed') else 1)
