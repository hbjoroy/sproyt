#!/usr/bin/env node
// Isolated image/database contract. It does not emulate Kubernetes controllers.
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { resolve } from 'node:path';
import { mkdir, writeFile } from 'node:fs/promises';
import { setTimeout as sleep } from 'node:timers/promises';
import { run, freePort } from './capacity-runtime.mjs';

const args = process.argv.slice(2);
assert(args.includes('--confirm-isolated'), 'requires --confirm-isolated; writes only to newly created labelled fixtures');
const value = (name, fallback) => args.includes(name) ? args[args.indexOf(name)+1] : fallback;
const image = value('--heart-image', 'oci.bjoroy.me/sproyt/heart@sha256:fe2eb544a14006504dbbb20e2c0a3489a0035eababefc5c93dba9f10c7ec1955');
assert(/^[a-z0-9./:_-]+@sha256:[a-f0-9]{64}$/.test(image), 'requires an immutable Heart image');
const output = resolve(value('--output', '.local/353-heart-budget-probe.json'));
assert(output.startsWith(resolve('.local')+'\\') || output.startsWith(resolve('.local')+'/'), 'output must stay in .local');
const nonce = randomUUID().replaceAll('-','').slice(0,12), prefix = `sproyt-heart-budget-${nonce}`;
const label = `sproyt.heart-budget-fixture=${nonce}`, containers = [], phases = [];
let network, postgres, sequence=0, locker, heldRequests=[];
const inspect = async id => JSON.parse(await run('wslc',['inspect',id]))[0];
const evidence = { schema:'sproyt.heart-rollout-image-contract.v1', at:new Date().toISOString(), image,
  roleLimit:8, runtimeProcesses:3, runtimePoolEach:2, runtimeV2Enabled:true, simultaneousMaintenanceProcesses:2,
  phases, errors:[], controllerRecreateTested:false, productionChanges:false, cleaned:false };
const save = async () => { await mkdir(resolve('.local'),{recursive:true}); await writeFile(output,JSON.stringify(evidence,null,2)+'\n'); };
async function create(name, imageName, options=[], command=[]) {
  const id = await run('wslc',['run','--detach','--name',`${prefix}-${name}`,'--label',label,'--network',network,...options,imageName,...command]);
  assert(/^[a-f0-9]{64}$/.test(id)); containers.push(id);
  const metadata=await inspect(id); assert.equal(metadata.Config.Labels['sproyt.heart-budget-fixture'],nonce);
  return id;
}
const pg = (sql,app='heart_budget_observer') => run('wslc',['exec',postgres,'env',`PGAPPNAME=${app}`,'psql','-X','-U','postgres','-d','heart_budget','-Atq','-v','ON_ERROR_STOP=1','-c',sql]);
const snapshot = async () => JSON.parse(await pg(`select json_build_object('total',count(*),'byApplication',(select json_agg(x) from (select application_name,count(*) as connections from pg_stat_activity where backend_type='client backend' and usename='heart_budget' group by application_name) x)) from pg_stat_activity where backend_type='client backend' and usename='heart_budget'`));
async function until(check, message, attempts=100) { for(let i=0;i<attempts;i++){if(await check())return;await sleep(100);}throw new Error(message); }
async function exited(id, success=true) {
  let state;
  await until(async()=>{state=(await inspect(id)).State;return !state.Running;},'fixture did not exit');
  if(success && state.ExitCode!==0)throw new Error(`Heart command failed: ${await run('wslc',['logs','--tail','8',id])}`);
  return state.ExitCode;
}
const runtimeOptions = ['--cpus','0.5','--memory','256M'];
function dbEnv(application,cap) { return ['--env',`DATABASE_URL=postgres://heart_budget:heart-budget-fixture@postgres:5432/heart_budget?application_name=${application}`,'--env',`HEART_DB_MAX_CONNECTIONS=${cap}`,'--env','BIND_ADDR=0.0.0.0:3000','--env','RUST_LOG=warn']; }
async function cli(name,command) {
  const id=await create(name,image,[...runtimeOptions,...dbEnv(`maintenance_${name}`,1),'--volume',`${resolve('helm/sproyt/definitions')}:/definitions:ro`],command);
  return id;
}
async function api(name) {
  const port=await freePort();const id=await create(name,image,[...runtimeOptions,...dbEnv(`runtime_${name}`,2),'--env','HEART_RUNTIME_V2_ENABLED=true','--publish',`127.0.0.1:${port}:3000`]);
  await until(async()=>{try{return (await fetch(`http://127.0.0.1:${port}/ready`,{signal:AbortSignal.timeout(1000)})).ok;}catch{return false;}},'Heart API not ready').catch(async error=>{throw new Error(error.message+': '+await run('wslc',['logs','--tail','10',id]));});
  const metadata=await inspect(id);assert.equal(metadata.HostConfig.Memory,256*1024**2);assert.equal(metadata.HostConfig.NanoCpus,500000000);
  return {id,port};
}
async function hold(apis) {
  locker=pg('begin; lock table heart.definition, public._sqlx_migrations in access exclusive mode; select pg_sleep(40); commit;','heart_budget_locker').catch(()=>{});
  await until(async()=>Number(await pg("select count(*) from pg_locks l join pg_stat_activity a on a.pid=l.pid where a.application_name='heart_budget_locker' and l.mode='AccessExclusiveLock' and l.granted"))>=2,'fixture lock not held');
  heldRequests=apis.flatMap(({port})=>[0,1].map(()=>fetch(`http://127.0.0.1:${port}/api/v1/definitions`,{signal:AbortSignal.timeout(30000)}).then(r=>r.status)));
  // Attach rejection handlers immediately; collect outcomes after releasing locks.
  heldRequests=heldRequests.map(p=>p.catch(error=>({error:error.message})));
  await until(async()=>{const s=await snapshot();return s.byApplication?.filter(a=>a.application_name.startsWith('runtime_')).reduce((n,a)=>n+a.connections,0)===apis.length*2;},'runtime pools did not reach declared limit');
}
async function release() {
  await pg("select pg_terminate_backend(pid) from pg_stat_activity where datname=current_database() and application_name='heart_budget_locker'");
  await locker;locker=null;const responses=await Promise.all(heldRequests);heldRequests=[];
  assert(responses.every(r=>r===200),'blocked runtime request failed');
}
async function parallelPhase(name,apis,command,ninth=false) {
  await hold(apis);const ids=[];for(const i of [0,1])ids.push(await cli(`${name}-${i}-${++sequence}`,command));
  const samples=[];
  await until(async()=>{const row=await snapshot();samples.push(row);assert(row.total<=8);return row.total===8;},`${name}: did not observe all six runtime and two maintenance connections`);
  for(let n=0;n<3;n++){samples.push(await snapshot());assert.equal(samples.at(-1).total,8);await sleep(100);}
  let ninthRejected=ninth ? false : null;
  if(ninth){
    try { await run('wslc',['exec',postgres,'env','PGPASSWORD=heart-budget-fixture','PGAPPNAME=unbudgeted_ninth','psql','-X','-h','127.0.0.1','-U','heart_budget','-d','heart_budget','-Atq','-c','select 1']); }
    catch(error){assert.match(error.message,/too many connections for role/);ninthRejected=true;}
    assert(ninthRejected,'fixture role did not enforce cap eight');
  }
  const stats=await run('wslc',['stats','--format','json',...apis.map(a=>a.id),...ids]);
  await release();await Promise.all(ids.map(id=>exited(id)));
  phases.push({name,observedPeak:Math.max(...samples.map(s=>s.total)),samples,ninthAttempted:ninth,ninthRejected,containerStats:stats});
  await save();console.log(`${name}: observed peak 8, runtime requests and maintenance completed`);
}

try {
  const metadata=JSON.parse(await run('wslc',['image','inspect',image]))[0];
  assert.deepEqual(metadata.Config.Entrypoint,['heart-api']);
  evidence.imageId=metadata.Id;evidence.imageRevision=metadata.Config.Labels?.['org.opencontainers.image.revision'];
  network=await run('wslc',['network','create','--label',label,prefix]);
  postgres=await create('pg','postgres:18.6-alpine',['--network-alias','postgres','--cpus','0.5','--memory','1536M','--env','POSTGRES_PASSWORD=heart-budget-fixture'],['-c','max_connections=32']);
  await until(async()=>{try{await run('wslc',['exec',postgres,'pg_isready','-h','127.0.0.1','-U','postgres']);return true;}catch{return false;}},'fixture postgres not ready');
  await run('wslc',['exec',postgres,'psql','-X','-U','postgres','-d','postgres','-Atq','-v','ON_ERROR_STOP=1','-c',"create role heart_budget login password 'heart-budget-fixture' connection limit 8"]);
  await run('wslc',['exec',postgres,'createdb','-U','postgres','-O','heart_budget','heart_budget']);
  evidence.databaseRole=JSON.parse(await pg("select json_build_object('superuser',rolsuper,'limit',rolconnlimit) from pg_roles where rolname='heart_budget'"));
  assert.equal(evidence.databaseRole.superuser,false);assert.equal(evidence.databaseRole.limit,8);
  await exited(await cli('initial-migrate',['migrate']));
  await exited(await cli('initial-event',['bootstrap-definition','/definitions/event-planning.yaml']));
  await exited(await cli('initial-pilot',['bootstrap-definition','/definitions/user-task-pilot.yaml']));
  let apis=[];for(const name of ['production-0','production-1','canary-0'])apis.push(await api(name));
  await parallelPhase('migration',apis,['migrate'],true);
  await parallelPhase('event-bootstrap',apis,['bootstrap-definition','/definitions/event-planning.yaml']);
  await parallelPhase('pilot-bootstrap',apis,['bootstrap-definition','/definitions/user-task-pilot.yaml']);
  const before=Number(await pg('select count(*) from heart.definition'));
  const failed=await cli('failed-bootstrap',['bootstrap-definition','/definitions/absent.yaml']);
  assert.notEqual(await exited(failed,false),0);
  await exited(await cli('retry-after-exit',['bootstrap-definition','/definitions/event-planning.yaml']));
  assert.equal(Number(await pg('select count(*) from heart.definition')),before);
  phases.push({name:'failed-bootstrap-retry',failedBeforeRetry:true,noDuplicateDefinitions:true,definitionCount:before});
  // Exercise real graceful drain, then replace. Controller ordering is validated separately.
  await hold(apis);const stopping=run('wslc',['stop','--time','10',apis[0].id,apis[1].id]);
  await sleep(500);const oldStillRunning=await Promise.all(apis.slice(0,2).map(async a=>(await inspect(a.id)).State.Running));
  assert(oldStillRunning.every(Boolean),'slow request did not hold old runtimes during graceful shutdown');
  await release();await stopping;
  const oldStates=[];for(const a of apis.slice(0,2))oldStates.push((await inspect(a.id)).State);
  assert(oldStates.every(s=>!s.Running && s.ExitCode===0),'runtime did not complete graceful shutdown successfully');
  const afterDrain=await snapshot();assert.equal(afterDrain.total,2,'old runtime database connections not released');
  apis=[await api('replacement-0'),await api('replacement-1'),apis[2]];
  phases.push({name:'controlled-drain-and-replace',oldStillRunningDuringDrain:oldStillRunning,oldExitCodes:oldStates.map(s=>s.ExitCode),connectionsBeforeReplacement:afterDrain.total,newRuntimeCount:apis.length,controllerGuaranteeTested:false});
  console.log('failed bootstrap, sequential retry and graceful drain contracts passed');
} catch(error){evidence.errors.push(error.message);throw error;}
finally {
  const discovered=(await run('wslc',['list','--all','--filter','label=sproyt.heart-budget-fixture='+nonce,'--no-trunc','--quiet'])).split(/\r?\n/).filter(id=>/^[a-f0-9]{64}$/.test(id));
  for(const id of [...new Set([...containers,...discovered])].toReversed()){
    const metadata=await inspect(id);assert.equal(metadata.Config.Labels['sproyt.heart-budget-fixture'],nonce,'refusing cleanup of unowned container');
    if(metadata.State.Running)await run('wslc',['stop','--time','2',id]);
    await run('wslc',['remove','--volumes',id]);
  }
  if(network){const metadata=JSON.parse(await run('wslc',['network','inspect',network]))[0];assert.equal(metadata.Labels['sproyt.heart-budget-fixture'],nonce);await run('wslc',['network','remove',network]);}
  evidence.cleaned=true;await save();console.log(`Evidence: ${output}`);
}
