"""Terminate only the confirmed test process, then verify saved-state recovery."""
import argparse,base64,hashlib,json,queue,subprocess,tarfile,threading,time,uuid
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
APP='com.motionstudio.editor'
RUNNER=APP+'.test/androidx.test.runner.AndroidJUnitRunner'
CLASS=APP+'.ProcessRecoveryTest'

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--serial',required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if args.serial=='emulator-5554':raise RuntimeError('This instance is reserved for UI work; use the separate acceptance device')
    shared=next(p for p in [ROOT,*ROOT.parents] if (p/'.tools/environment.json').exists())
    config=json.loads((shared/'.tools/environment.json').read_text())
    adb=[str(Path(config['sdk'])/'platform-tools/adb.exe'),'-s',args.serial]
    args.output.mkdir(parents=True,exist_ok=True)
    def run(*parameters,check=True):return subprocess.run([*adb,*parameters],capture_output=True,check=check)
    def saved_user():
        r=run('exec-out','run-as',APP,'cat','files/studio/default/project.json',check=False)
        return r.stdout if r.returncode==0 and r.stdout.startswith(b'{') else None
    before=saved_user();token='process-recovery-'+uuid.uuid4().hex
    command=[*adb,'shell','am','instrument','-w','-e','class',CLASS+'#prepareAndWaitForExternalProcessTermination','-e','recoveryToken',token,RUNNER]
    process=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
    messages=queue.Queue();lines=[]
    def read():
        for raw in iter(process.stdout.readline,b''):
            text=raw.decode('utf-8',errors='replace');lines.append(text);messages.put(text)
    reader=threading.Thread(target=read,daemon=True);reader.start()
    ready=None;deadline=time.monotonic()+90
    try:
        while time.monotonic()<deadline:
            try:text=messages.get(timeout=.5)
            except queue.Empty:
                if process.poll() is not None:break
                continue
            if 'INSTRUMENTATION_STATUS: recoveryReady=' in text:
                payload=text.split('recoveryReady=',1)[1].strip();ready=json.loads(base64.b64decode(payload));break
        if ready is None:raise RuntimeError('Prepare phase failed; no confirmed ready process')
        if ready['token']!=token:raise RuntimeError('Wrong test directory reported')
        pids=run('shell','pidof',APP).stdout.decode().split()
        if str(ready['pid']) not in pids:raise RuntimeError('The prepared application process is no longer live')
        run('shell','am','force-stop',APP)
        process.wait(timeout=30);reader.join(timeout=5)
        if run('shell','pidof',APP,check=False).stdout.strip():raise RuntimeError('Target process did not terminate')
        recovered=run('shell','am','instrument','-w','-e','class',CLASS+'#reopenAfterProcessTerminationPreservesProjectAssetsAndPixels','-e','recoveryToken',token,RUNNER,check=False)
        result=(recovered.stdout+recovered.stderr).decode('utf-8',errors='replace')
        (args.output/'recovery-instrumentation.txt').write_text(result,encoding='utf-8')
        print(result,flush=True)
        if recovered.returncode!=0 or 'OK (1 test)' not in result or 'FAILURES!!!' in result:raise RuntimeError('Recovery verification failed')
        archive=args.output/'recovery.tar'
        with archive.open('wb') as stream:
            subprocess.run([*adb,'exec-out','run-as',APP,'tar','-c','-f','-','files/acceptance/'+token],stdout=stream,check=True)
        with tarfile.open(archive) as bundle:bundle.extractall(args.output/'recovery',filter='data')
        after=saved_user()
        if before is not None and before!=after:raise RuntimeError('User project changed')
        report={'deviceSerial':args.serial,'preparedPid':ready['pid'],'intentionalExternalTermination':True,'recoveryPassed':True,
                'userProjectPreserved':before is None or before==after,'beforeSha256':hashlib.sha256(before).hexdigest() if before else None,'afterSha256':hashlib.sha256(after).hexdigest() if after else None,
                'scope':'Confirmed saved test process forcibly stopped; recovery test uses a new process. Not power-loss testing during an uncommitted save.'}
        (args.output/'process-recovery-summary.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    finally:
        if process.poll() is None:process.terminate();process.wait(timeout=10)
        reader.join(timeout=5)
        (args.output/'prepare-instrumentation.txt').write_text(''.join(lines),encoding='utf-8')
        run('shell','am','start','-n',APP+'/.MainActivity',check=False)

if __name__=='__main__':main()
