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
 phases=('full-caller',)
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
  for fd in fds:os.close(fd)
  os.rmdir(state);os.rmdir(q)
  sys.path.insert(0,os.environ['TERRAFORM_TEST_PACKAGING'])
  import nuc_claim_target as targetmod,nuc_claim_run as runner,das_unit_start_fence as fence
  import types
  class FakeTarget:
   def __init__(self, *, stopped=False):
    self.opened=[];self.paths={}
    for key,path in ((runner.BACKUP_PARENT,backup),(runner.SOURCE_PARENT,sp),(runner.DESTINATION_PARENT,dp),(runner.JOURNAL_PARENT,jdir),('/srv/dasobjectstore/hdd/qnap-1058',dm)):
     fd=os.open(path,os.O_RDONLY|os.O_DIRECTORY);self.opened.append(fd);self.paths[key]=types.SimpleNamespace(fds=[fd])
    self.connection=sqlite3.connect(dbpath,isolation_level=None);self.daemon_stopped=stopped
   def recheck(self):pass
   def observe_stopped(self):self.daemon_stopped=True
   def close(self):
    self.connection.close()
    for fd in self.opened:os.close(fd)
  targetmod.HeldTarget=FakeTarget;runner.HeldTarget=FakeTarget
  runner.STATE_PARENT=str(jr);runner.STATE_NAME='state';runner.QUARANTINE_NAME='q0';runner.JOURNAL_SHA=journal_sha;runner.JOURNAL_PARENT=str(jdir)
  real_acquire=fence.DasClientStartFence.acquire;real_recheck=fence.DasClientStartFence.recheck;real_promote=fence.DasClientStartFence.promote_after_recovery;real_recover=fence.DasClientStartFence.recover_client_held;real_promoted=fence.DasClientStartFence.recover_promoted
  def fake_acquire(self):self.fd=os.open('/dev/null',os.O_RDONLY);return self
  def fake_recheck(self):pass
  def fake_promoted(self):
   self.fd=os.open('/dev/null',os.O_RDONLY);self.journal_kind='das-promoted-client-fence-v1';return self
  def fake_promote(self,proof):
   proof.recheck();proof.target.observe_stopped();proof.recheck();self.journal_kind='das-promoted-client-fence-v1';return self
  fence.DasClientStartFence.recover_promoted=fake_promoted;fence.DasClientStartFence.acquire=fake_acquire;fence.DasClientStartFence.recover_client_held=fake_acquire;fence.DasClientStartFence.recheck=fake_recheck;fence.DasClientStartFence.promote_after_recovery=fake_promote
  writer=os.fork()
  if writer==0:
   try:
    os.setgroups([116,122,971,972]);os.setgid(972);os.setuid(994)
    while os.stat(journal).st_uid!=0:time.sleep(.05)
    time.sleep(.5)
    for k in (1,2):
     tmp=jdir/('report-'+str(k));tmp.write_text('report '+str(k));os.replace(tmp,jdir/'latest.json');time.sleep(.4)
    os._exit(0)
   except BaseException:os._exit(1)
  try:
   interrupt=os.environ.get('DAS_FULL_INTERRUPT')
   if interrupt:
    child=os.fork()
    if child==0:
     original_write=recovery.IntegratedClaimRecovery._write
     if interrupt.startswith('restore:'):
      native_rename=os.rename
      def kill_restore(src,dst,*args,**kwargs):
       result=native_rename(src,dst,*args,**kwargs)
       if src=='restore.new':
        data=json.loads((state/'restore.json').read_text())
        if data['phase']==interrupt.split(':',1)[1]:os.kill(os.getpid(),signal.SIGKILL)
       return result
      os.rename=kill_restore
     def kill_at(self,value):
      original_write(self,value)
      if value==interrupt:os.kill(os.getpid(),signal.SIGKILL)
     recovery.IntegratedClaimRecovery._write=kill_at
     try:
      with runner.hold_claim_recovery(stop_and_restore=True):pass
      os._exit(9)
     except BaseException:os._exit(8)
    _,status=os.waitpid(child,0)
    if not os.WIFSIGNALED(status) or os.WTERMSIG(status)!=signal.SIGKILL:raise RuntimeError('caller interruption failed')
    if interrupt.startswith('restore:'):runner._promoted_hold_exists=lambda:True
   with runner.hold_claim_recovery(stop_and_restore=True) as live:
    live.recheck(); result=live.result
    if result['restore']!='restored-daemon-still-fenced' or sf.read_bytes()!=b'complete' or (dm/'q0'/'partial').read_bytes()!=b'partial' or journal.exists() or not (state/'original-journal').is_file():raise RuntimeError('full caller outcome differs')
    if live.fence.fd is None:raise RuntimeError('holder lock closed before consumer')
   _,status=os.waitpid(writer,0)
   if not os.WIFEXITED(status) or os.WEXITSTATUS(status)!=0:raise RuntimeError('fixture reporter failed')
   results[phase]=True;results['all_passed']=True
  finally:
   fence.DasClientStartFence.acquire=real_acquire;fence.DasClientStartFence.recheck=real_recheck;fence.DasClientStartFence.promote_after_recovery=real_promote;fence.DasClientStartFence.recover_client_held=real_recover;fence.DasClientStartFence.recover_promoted=real_promoted
finally:
 for mount in reversed(mounts):subprocess.run(['/usr/bin/umount',str(mount)],check=True);os.rmdir(mount)
 data=(json.dumps({'results':results,'details':details},sort_keys=True)+'\n').encode();fd=os.open(root/'result.json',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o400);os.write(fd,data);os.fsync(fd);os.close(fd)
print(json.dumps({'root':str(root),'results':results},sort_keys=True));sys.exit(0 if results.get('all_passed') else 1)
