// Runs the external adapter profile against isolated local protocol services.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
const suffix=randomBytes(6).toString('hex'), names=[];
const access='brain'+suffix, secret=randomBytes(24).toString('hex');
const env={...process.env,BRAIN_TEST_ACCESS:access,BRAIN_TEST_SECRET:secret,POSTGRES_PASSWORD:secret,RUSTFS_ACCESS_KEY:access,RUSTFS_SECRET_KEY:secret};
function run(cmd,args,options={}) {const r=spawnSync(cmd,args,{encoding:'utf8',windowsHide:true,env,timeout:240000,...options});assert.ifError(r.error);assert.equal(r.status,0,r.stderr);return r.stdout?.trim();}
function start(service,image,port,extra=[]) {
 const name=`brain-adapter-${suffix}-${service}`;
 run('docker',['run','--detach','--rm','--name',name,'--publish',`127.0.0.1::${port}`,...extra,image]);names.push(name);
 return {name,address:run('docker',['port',name,`${port}/tcp`]).split('\n')[0]};
}
try {
 const pg=start('postgres','postgres:18@sha256:b9f6b8ff03b62f3cfd9d9ba3fe56226a9991c79d3fb46fd46536263bf02be553',5432,['--env','POSTGRES_PASSWORD']);
 const s3=start('s3','rustfs/rustfs:1.0.0-alpha.77@sha256:f761246690fdf92fc951c90c12ce4050994c923fb988e3840f072c7d9ee11a63',9000,['--env','RUSTFS_ACCESS_KEY','--env','RUSTFS_SECRET_KEY']);
 const ddb=start('ddb','amazon/dynamodb-local@sha256:6ada631ec7aa5fc83446855eb17233e3dad184fec47477e8140c6c569cfe0c86',8000);
 const deadline=Date.now()+90000;
 for(const service of [s3,ddb]) {
  while(true) {
   try {const r=await fetch(`http://${service.address}/`,{signal:AbortSignal.timeout(1000)});await r.body?.cancel();break;}catch(error){if(Date.now()>deadline)throw error;await delay(250);}
  }
 }
 while(true){const r=spawnSync('docker',['exec',pg.name,'pg_isready','-U','postgres'],{encoding:'utf8',windowsHide:true});if(r.status===0)break;if(Date.now()>deadline)throw Error('PostgreSQL did not become ready');await delay(250);}
 env.BRAIN_TEST_POSTGRES=`postgres://postgres:${secret}@${pg.address}/postgres?sslmode=disable`;
 env.BRAIN_TEST_S3=`http://${s3.address}`;env.BRAIN_TEST_DDB=`http://${ddb.address}`;
 run('go',['test','-count=1','-tags','external','./services/storage/internal/adapters','-timeout','3m'],{stdio:'inherit'});
 run('go',['test','-count=1','-tags','postgres','./services/brain/internal/archive','-timeout','2m'],{stdio:'inherit'});
 console.log('External adapter contracts passed: PostgreSQL, S3-compatible RustFS and DynamoDB Local.');
} finally {for(const name of names.reverse())run('docker',['rm','--force',name]);}
