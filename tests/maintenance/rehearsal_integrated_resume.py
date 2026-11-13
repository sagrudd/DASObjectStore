import array,fcntl,hashlib,json,os,pathlib,sqlite3,subprocess,sys,tempfile
sys.path.insert(0,str(pathlib.Path(__file__).resolve().parents[2]/'tools'))
import integrated_claim_recovery as r, recovered_journal_marker as mkr
root=pathlib.Path(sys.argv[1])
if os.geteuid()!=0 or root.parent!=pathlib.Path('/data/mnemosyne-build-output-1000') or not root.name.startswith('das-integration-fixture-') or root.is_symlink():raise SystemExit('invalid disposable fixture root')
m=pathlib.Path(tempfile.mkdtemp(prefix='das-integrated-resume-',dir='/tmp'));b=pathlib.Path(tempfile.mkdtemp(prefix='das-integrated-backup-',dir='/tmp'));os.chmod(m,0o755)
subprocess.run(['/usr/bin/mount','-o','loop,nosuid,nodev',str(root/'ext4.img'),str(m)],check=True)
subprocess.run(['/usr/bin/mount','-o','loop,nosuid,nodev',str(root/'backup-ext4.img'),str(b)],check=True)
fds=[];out={}
try:
 def op(path):
  fd=os.open(path,os.O_RDONLY|os.O_DIRECTORY);fds.append(fd);return fd
 bfd=op(b);statefd=op(m/'state');spfd=op(m/'source');dpfd=op(m/'destination');qfd=op(m/'quarantine')
 r.EXPECTED_BACKUP_RECEIPT_SHA256=hashlib.sha256((b/'receipt.json').read_bytes()).hexdigest()
 mkr._EXPECTED_JOURNAL=hashlib.sha256((m/'journal/operation.json').read_bytes()).hexdigest()
 backups=r.VerifiedBackups(bfd);held=mkr.RecoveredJournalMarker(m/'journal',statefd);conn=sqlite3.connect(m/'live.sqlite',isolation_level=None)
 before=conn.execute("SELECT state FROM disk_capacity_claims WHERE claim_id='claim-1'").fetchone()
 result=r.resume_quarantined(held,backups,spfd,dpfd,qfd,statefd,conn)
 after=conn.execute("SELECT state FROM disk_capacity_claims WHERE claim_id='claim-1'").fetchone()
 out={'before':before,'after':after,'result':result,'marker_present':(m/'journal/operation.json').is_file(),'source_unchanged':(m/'source/payload').read_bytes()==b'complete','partial_preserved':(m/'quarantine/partial').read_bytes()==b'partial','destination_absent':not (m/'destination/payload').exists(),'state_phase':json.loads((m/'state/recovery.json').read_text())['phase']}
 print(json.dumps(out,sort_keys=True))
 conn.close();held.close();backups.close()
 # Disposable fixture cleanup only; never a live release procedure.
 for path in (m/'source/payload',m/'journal/operation.json'):
  f=os.open(path,os.O_RDONLY);v=array.array('L',[0]);fcntl.ioctl(f,0x80086601,v,True);v[0]&=~0x10;fcntl.ioctl(f,0x40086602,v,True);os.close(f)
finally:
 for fd in fds:
  try:os.close(fd)
  except OSError:pass
 subprocess.run(['/usr/bin/umount',str(b)],check=True);subprocess.run(['/usr/bin/umount',str(m)],check=True);os.rmdir(b);os.rmdir(m)
