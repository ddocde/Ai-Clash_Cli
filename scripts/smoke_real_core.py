#!/usr/bin/env python3
"""Opt-in isolated no-TUN smoke: python scripts/smoke_real_core.py /path/to/mihomo.
Requires curl and free loopback ports 17890, 19090, 18081. Never installs services.
"""
import subprocess,json,pathlib,time,os,signal,http.server,threading,tempfile,sys,socket
if len(sys.argv) != 2:
 raise SystemExit('Usage: python scripts/smoke_real_core.py /absolute/path/to/mihomo')
for port in (17890,19090,18081):
 with socket.socket() as probe:
  probe.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
  probe.bind(('127.0.0.1',port))
root=pathlib.Path(__file__).resolve().parents[1];cli=os.environ.get('CLASH_CLI_BINARY',str(root/'target/debug/clash-cli'));d=pathlib.Path(tempfile.mkdtemp(prefix='clash-cli-real-'))
def run(*args,ok=True):
 p=subprocess.run([cli,'--data-dir',str(d),*args,'--json'],text=True,capture_output=True,timeout=40)
 if ok and p.returncode:raise RuntimeError((args,p.returncode,p.stdout,p.stderr))
 if ok:return json.loads(p.stdout)
 return p
run('config','--mixed-port','17890','--controller-port','19090')
run('core','install','--path',sys.argv[1])
f=d/'source.yaml';f.write_text('proxies: []\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT, REJECT]\nrules:\n  - MATCH,PROXY\n')
run('profile','add','smoke',str(f));run('profile','use','smoke')
log=open(d/'run.log','w');p=subprocess.Popen([cli,'--data-dir',str(d),'run'],stdout=log,stderr=log)
class Handler(http.server.BaseHTTPRequestHandler):
 def do_GET(self):self.send_response(204);self.end_headers()
 def log_message(self,*a):pass
server=http.server.HTTPServer(('127.0.0.1',18081),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
try:
 for i in range(100):
  result=run('status',ok=False)
  if result.returncode==0:break
  if p.poll() is not None:raise RuntimeError((d/'run.log').read_text())
  time.sleep(.1)
 else:raise RuntimeError('not ready')
 assert run('group','list')
 run('node','select','DIRECT','--group','PROXY')
 results=run('node','test','DIRECT','--url','http://127.0.0.1:18081/','--timeout','3000');assert results[0]['status']=='ok',results
 q=subprocess.run(['curl','--silent','--show-error','--fail','--noproxy','','--proxy','http://127.0.0.1:17890','http://127.0.0.1:18081/'],timeout=10);assert q.returncode==0
 run('mode','global');run('mode','rule')
 run('node','select','REJECT','--group','PROXY')
 p.send_signal(signal.SIGTERM);p.wait(timeout=15)
 p=subprocess.Popen([cli,'--data-dir',str(d),'run'],stdout=log,stderr=log)
 for i in range(100):
  result=run('node','list',ok=False)
  if result.returncode==0 and json.loads(result.stdout)['proxies']['PROXY']['now']=='REJECT':break
  time.sleep(.1)
 else:raise RuntimeError('selection not restored after restart')
 run('node','select','DIRECT','--group','PROXY')
 old=f.read_text();f.write_text('proxies: [broken]\n')
 assert run('profile','update','smoke',ok=False).returncode != 0
 assert run('node','list')['proxies']['PROXY']['now']=='DIRECT'
 f.write_text(old)
 f.write_text(f.read_text()+'# updated\n');run('profile','update','smoke')
 run('node','import','ss://YWVzLTEyOC1nY206cGFzcw@example.com:8388#fixture','--profile','links')
 assert 'links' in run('profile','list')['profiles']
 print(json.dumps({'real_core_smoke':'passed','dir':str(d),'checks':['install offline','profile validate/use','startup','groups','select','latency','HTTP proxy','mode reload','profile update','SS link import','restart selection persistence','invalid update rollback','graceful termination']}))
finally:
 if p.poll() is None:p.send_signal(signal.SIGTERM);p.wait(timeout=15)
 server.shutdown();log.close()
