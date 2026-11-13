import array,fcntl,hashlib,json,os,pathlib,signal,sqlite3,stat,subprocess,sys,tempfile,time
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[2]/'tools'))
import sticky_journal_parent as parent,housekeeping_generation_observer as observer,original_journal_marker as marker,integrated_claim_recovery as recovery
root=pathlib.Path(sys.argv[1])
if os.geteuid()!=0 or root.parent!=pathlib.Path('/data/mnemosyne-build-output-1000') or not root.name.startswith('das-integration-fixture-'):raise SystemExit('invalid disposable fixture root')
root.mkdir(mode=0o700)
image=root/'ext4.img';fd=os.open(image,os.O_CREAT|os.O_EXCL|os.O_RDWR,0o600);os.ftruncate(fd,128*1024*1024);os.close(fd)
subprocess.run(['/usr/sbin/mkfs.ext4','-q','-F',str(image)],check=True)
m=pathlib.Path(tempfile.mkdtemp(prefix='das-integrated-',dir='/tmp'));os.chmod(m,0o755);subprocess.run(['/usr/bin/mount','-o','loop,nosuid,nodev',str(image),str(m)],check=True)
out={}; openfds=[]; backup_mounted=False
try:
 jdir=m/'journal';jdir.mkdir();os.chown(jdir,994,972);os.chmod(jdir,0o750)
 state=m/'state';state.mkdir(mode=0o700);q=m/'quarantine';q.mkdir(mode=0o700);backup=m/'backups';backup.mkdir(mode=0o700)
 backup_image=root/'backup-ext4.img';bf=os.open(backup_image,os.O_CREAT|os.O_EXCL|os.O_RDWR,0o600);os.ftruncate(bf,64*1024*1024);os.close(bf)
 subprocess.run(['/usr/sbin/mkfs.ext4','-q','-F',str(backup_image)],check=True)
 subprocess.run(['/usr/bin/mount','-o','loop,nosuid,nodev',str(backup_image),str(backup)],check=True);os.chmod(backup,0o700);backup_mounted=True
 sp=m/'source';dp=m/'destination'
 for p in (sp,dp):p.mkdir();os.chown(p,994,972);os.chmod(p,0o750)
 sf=sp/'payload';df=dp/'payload';sf.write_bytes(b'complete');df.write_bytes(b'partial');os.chown(sf,994,972);os.chmod(sf,0o640);os.chown(df,994,972);os.chmod(df,0o660)
 ss,ds=sf.stat(),df.stat();source_sha=hashlib.sha256(b'complete').hexdigest();partial_sha=hashlib.sha256(b'partial').hexdigest()
 candidate={'placement_id':'placement','object_id':'object','store_id':'store','disk_id':'source-disk','disk_state':'Watch','relative_path':'objects/payload','size_bytes':8,'content_hash':source_sha,'verified_at_utc':'2026-01-01T00:00:00Z','existing_disk_ids':['source-disk','other-disk']}
 claim=dict(zip(('claim_id','claim_kind','owner_id','request_id','request_digest','disk_id','reserved_bytes','consumed_bytes','created_at_utc'),('claim-1','repair','owner','request','digest','disk',8,0,'2026-01-01T00:00:00Z')))
 intent={'claim':claim,'candidate':candidate,'source_sha256':source_sha,'original_updated_at':'2026-01-01T00:00:00Z','original_lease':[None,None],'release_time':'2026-01-02T00:00:00Z'}
 journal=jdir/'operation.json';journal.write_text(json.dumps({'schema':'dasobjectstore.storage_assurance.operation.v1','operation_id':'fixture','action':'rotate','phase':'claimed','candidate':candidate,'destination_disk_id':'disk','destination_relative_path':'objects/payload','claim_owner':'owner','created_at_utc':'t','updated_at_utc':'t'}));os.chown(journal,994,972);os.chmod(journal,0o600)
 rows=[{'name':'source','size':8,'sha256':source_sha,'device':ss.st_dev,'inode':ss.st_ino},{'name':'partial','size':7,'sha256':partial_sha,'device':ds.st_dev,'inode':ds.st_ino}]
 for name,data in (('source',b'complete'),('partial',b'partial')):(backup/name).write_bytes(data);os.chmod(backup/name,0o600)
 receipt=backup/'receipt.json';receipt.write_text(json.dumps(rows));os.chmod(receipt,0o600);recovery.EXPECTED_BACKUP_RECEIPT_SHA256=hashlib.sha256(receipt.read_bytes()).hexdigest()
 bfd=os.open(backup,os.O_RDONLY|os.O_DIRECTORY);pfd=os.open(jdir,os.O_RDONLY|os.O_DIRECTORY);statefd=os.open(state,os.O_RDONLY|os.O_DIRECTORY);qfd=os.open(q,os.O_RDONLY|os.O_DIRECTORY)
 spfd=os.open(sp,os.O_RDONLY|os.O_DIRECTORY);dpfd=os.open(dp,os.O_RDONLY|os.O_DIRECTORY);sfd=os.open(sf,os.O_RDONLY);dfd=os.open(df,os.O_RDONLY);openfds=[bfd,pfd,statefd,qfd,spfd,dpfd,sfd,dfd]
 backups=recovery.VerifiedBackups(bfd);ps=os.fstat(pfd);js=journal.stat()
 intent['filesystem']={}
 for name,dir_fd,file_fd in (('source',spfd,sfd),('partial',dpfd,dfd)):
  d=os.fstat(dir_fd);f=os.fstat(file_fd)
  intent['filesystem'][name]={'parent_device':d.st_dev,'parent_inode':d.st_ino,'parent_uid':d.st_uid,'parent_gid':d.st_gid,'parent_mode':stat.S_IMODE(d.st_mode),'parent_acl':parent._acl(dir_fd),'file_uid':f.st_uid,'file_gid':f.st_gid,'file_mode':stat.S_IMODE(f.st_mode),'file_acl':parent._acl(file_fd),'file_flags':marker._flags(file_fd)}
 recovery.EXPECTED_ORIGINAL_JOURNAL_SHA256=hashlib.sha256(journal.read_bytes()).hexdigest()
 guard=parent.StickyJournalParent(pfd,statefd,(ps.st_dev,ps.st_ino),(js.st_dev,js.st_ino),recovery.EXPECTED_ORIGINAL_JOURNAL_SHA256);guard.acquire()
 obs=observer.ReportGenerationObserver(jdir);mark=marker.OriginalJournalMarker(guard,obs);mark.install()
 source=recovery.Payload(spfd,sfd,backups.rows['source']);partial=recovery.Payload(dpfd,dfd,backups.rows['partial'])
 db=sqlite3.connect(m/'live.sqlite',isolation_level=None)
 schema=(pathlib.Path(__file__).resolve().parents[2]/'crates/dasobjectstore-metadata/src/schema.rs').read_text().split('pub const LIVE_SCHEMA_SQL: &str = r#"',1)[1].split('"#;',1)[0]
 db.executescript(schema);db.execute('PRAGMA foreign_keys=OFF')
 db.execute("INSERT INTO objects(object_id,store_id,state,size_bytes,content_hash,created_at_utc,updated_at_utc) VALUES('object','store','HddCopyVerified',8,?,'t','t')",(source_sha,))
 db.execute("INSERT INTO placements(placement_id,object_id,disk_id,relative_path,content_hash,created_at_utc) VALUES('placement','object','source-disk','objects/payload',?,'t')",(source_sha,))
 db.execute("INSERT INTO disk_capacity_claims(claim_id,claim_kind,owner_id,request_id,request_digest,disk_id,reserved_bytes,consumed_bytes,created_at_utc,state,released_at_utc,updated_at_utc,lease_owner,lease_expires_at_utc) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",tuple(claim[k] for k in ('claim_id','claim_kind','owner_id','request_id','request_digest','disk_id','reserved_bytes','consumed_bytes','created_at_utc'))+('active',None,'2026-01-01T00:00:00Z',None,None))
 writer=os.fork()
 if writer==0:
  os.setgroups([116,122,971,972]);os.setgid(972);os.setuid(994)
  for i in (1,2):
   time.sleep(.5);tmp=jdir/('report-'+str(i));tmp.write_text('report '+str(i));os.replace(tmp,jdir/'latest.json')
  os._exit(0)
 coord=recovery.IntegratedClaimRecovery(mark,backups,source,partial,statefd,qfd,db,intent)
 original_write=coord._write
 def crash_after_quarantine(phase):
  original_write(phase)
  if phase=='quarantined':os.kill(os.getpid(),signal.SIGKILL)
 coord._write=crash_after_quarantine
 try:result=coord.run(timeout=10)
 finally:os.waitpid(writer,0)
 out['committed'] = result['state']=='committed-marker-retained' and db.execute("SELECT state FROM disk_capacity_claims WHERE claim_id='claim-1'").fetchone()==('released',)
 out['original_journal_present']=journal.exists() and journal.read_text().find('fixture')>=0 and bool(marker._flags(guard.journal_fd)&0x10)
 out['source_protected']=sf.read_bytes()==b'complete' and bool(marker._flags(sfd)&0x10)
 out['partial_quarantined']=not df.exists() and (q/'partial').read_bytes()==b'partial'
 out['state_committed']=json.loads((state/'recovery.json').read_text())['phase']=='committed'
 out['all_passed']=all(out.values())
 coord.close();mark.close();obs.close();guard.close();backups.close();db.close()
 for path in (sf,journal):
  f=os.open(path,os.O_RDONLY);v=array.array('L',[0]);fcntl.ioctl(f,0x80086601,v,True);v[0]&=~0x10;fcntl.ioctl(f,0x40086602,v,True);os.close(f)
finally:
 if 'backups' in locals() and backups.dirfd is not None:backups.close()
 for fd in openfds:
  try:os.close(fd)
  except OSError:pass
 if backup_mounted:subprocess.run(['/usr/bin/umount',str(backup)],check=True)
 subprocess.run(['/usr/bin/umount',str(m)],check=True);os.rmdir(m)
 data=(json.dumps(out,sort_keys=True)+'\n').encode();fd=os.open(root/'result.json',os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o400);os.write(fd,data);os.fsync(fd);os.close(fd)
print(json.dumps(out,sort_keys=True))
