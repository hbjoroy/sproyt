#!/usr/bin/env node
// Replays the real event-planning definition against an isolated, pinned Heart.
import assert from 'node:assert/strict';
import {randomUUID, createHash} from 'node:crypto';
import {resolve, relative, isAbsolute, sep} from 'node:path';
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import {once} from 'node:events';
import {setTimeout as sleep} from 'node:timers/promises';
import {run, freePort, applicationEnv, startApi, openChat} from './capacity-runtime.mjs';

const args=process.argv.slice(2);
assert(args.includes('--confirm-isolated'), 'requires --confirm-isolated; creates only labelled disposable fixtures');
const option=(key,fallback)=>args.includes(key)?args[args.indexOf(key)+1]:fallback;
assert(args.includes('--binary'), 'requires the freshly built Sprøyt binary');
const binary=resolve(option('--binary'));
const output=resolve(option('--output','.local/heart-event-planning-result.json'));
const outputRelative=relative(resolve('.local'),output);
assert(outputRelative && !isAbsolute(outputRelative) && !outputRelative.split(sep).includes('..'), 'output must be inside .local');
const image='oci.bjoroy.me/sproyt/heart@sha256:fe2eb544a14006504dbbb20e2c0a3489a0035eababefc5c93dba9f10c7ec1955';
const nonce=randomUUID().replaceAll('-','').slice(0,12), prefix=`sproyt-heart-event-${nonce}`;
const labelKey='sproyt.heart-event-fixture', label=`${labelKey}=${nonce}`;
const containers=[];let network,postgres,heart,api,chat;
const evidence={schema:'sproyt.heart-event-planning.v1',at:new Date().toISOString(),heartImage:image,
 application:'fresh native Windows ARM64 Sprøyt build; real PostgreSQL and ARM64 Heart containers',
 binarySha256:createHash('sha256').update(await readFile(binary)).digest('hex'),
 definitionSha256:createHash('sha256').update(await readFile('helm/sproyt/definitions/event-planning.yaml')).digest('hex'),
 productionChanges:false,kubernetesControllerRetested:false,scenarios:[],errors:[],cleaned:false};
evidence.sourceSha256=Object.fromEntries(await Promise.all(['src/process.rs','src/db/postgres.rs','src/db/sqlite.rs','src/db/mod.rs'].map(async path=>[path,createHash('sha256').update(await readFile(path)).digest('hex')])));
const inspect=async id=>JSON.parse(await run('wslc',['inspect',id]))[0];
const save=async()=>{await mkdir(resolve('.local'),{recursive:true});await writeFile(output,JSON.stringify(evidence,null,2)+'\n');};
async function until(check,message,attempts=160){for(let i=0;i<attempts;i++){const value=await check();if(value)return value;await sleep(250);}throw new Error(message);}
async function create(name,imageName,options=[],command=[]){
 const id=await run('wslc',['run','--detach','--name',`${prefix}-${name}`,'--label',label,'--network',network,...options,imageName,...command]);
 assert(/^[a-f0-9]{64}$/.test(id));containers.push(id);assert.equal((await inspect(id)).Config.Labels[labelKey],nonce);return id;
}
const pg=(db,sql)=>run('wslc',['exec',postgres,'psql','-X','-U','postgres','-d',db,'-Atq','-v','ON_ERROR_STOP=1','-c',sql]);
const pgJson=async(db,sql)=>JSON.parse(await pg(db,sql));
const heartEnv=['--cpus','0.5','--memory','256M','--env','DATABASE_URL=postgres://heart_budget:heart-fixture@postgres:5432/heart_budget','--env','HEART_DB_MAX_CONNECTIONS=2','--env','HEART_RUNTIME_V2_ENABLED=true','--env','BIND_ADDR=0.0.0.0:3000','--env','RUST_LOG=warn'];
async function exited(id){await until(async()=>!(await inspect(id)).State.Running,'Heart CLI did not exit');const state=(await inspect(id)).State;assert.equal(state.ExitCode,0,await run('wslc',['logs','--tail','20',id]));}
async function stopApi(){if(api && api.exitCode===null){const ended=once(api,'exit');api.kill();await ended;}api=undefined;}
try{
 const metadata=JSON.parse(await run('wslc',['image','inspect',image]))[0];assert.deepEqual(metadata.Config.Entrypoint,['heart-api']);
 evidence.heartImageId=metadata.Id;evidence.heartRevision=metadata.Config.Labels?.['org.opencontainers.image.revision'];
 network=await run('wslc',['network','create','--label',label,prefix]);
 const pgPort=await freePort(),heartPort=await freePort(),apiPort=await freePort();
 postgres=await create('pg','postgres:18.6-alpine',['--network-alias','postgres','--cpus','0.5','--memory','1536M','--publish',`127.0.0.1:${pgPort}:5432`,'--env','POSTGRES_PASSWORD=heart-fixture'],['-c','max_connections=32']);
 await until(async()=>{try{await run('wslc',['exec',postgres,'pg_isready','-h','127.0.0.1','-U','postgres']);return true;}catch{return false;}},'PostgreSQL not ready');
 await pg('postgres',"create role heart_budget login password 'heart-fixture' connection limit 8; create role sproyt login password 'capacity-fixture';");
 for(const [db,owner] of [['heart_budget','heart_budget'],['sproyt_fixture','sproyt'],['contract_fixture','sproyt']])await run('wslc',['exec',postgres,'createdb','-U','postgres','-O',owner,db]);
 await exited(await create('migration',image,heartEnv,['migrate']));
 await exited(await create('definition',image,[...heartEnv,'--volume',`${resolve('helm/sproyt/definitions')}:/definitions:ro`],['bootstrap-definition','/definitions/event-planning.yaml']));
 if(args.includes('--contract-binary')){
  const testBinary=resolve(option('--contract-binary'));
  const env={...process.env,SPROYT_POSTGRES_TEST_URL:`postgres://sproyt:capacity-fixture@127.0.0.1:${pgPort}/contract_fixture`};
  const logs=await run(testBinary,['db::postgres::tests::postgres_repository_persists_and_reads_messages','--exact','--nocapture'],{env,timeoutMs:120000});
  assert.match(logs,/1 passed; 0 failed/);evidence.postgresRepositoryContract='passed';
 }
 heart=await create('heart',image,[...heartEnv,'--publish',`127.0.0.1:${heartPort}:3000`]);
 const ready=async()=>{try{return (await fetch(`http://127.0.0.1:${heartPort}/ready`,{signal:AbortSignal.timeout(1000)})).ok;}catch{return false;}};
 await until(ready,'Heart not ready');
 const env={...applicationEnv('sproyt_fixture',pgPort,apiPort,0,1),SPROYT_PROCESS_OUTBOX_ENABLED:'true',SPROYT_HEART_URL:`http://127.0.0.1:${heartPort}`};
 await run(binary,['migrate'],{env,timeoutMs:120000});api=await startApi(binary,env);
 const owner='event-fixture-owner',base=`http://127.0.0.1:${apiPort}`;
 const request=async(path,body,method='POST')=>{
  const response=await fetch(base+path+'?participant='+owner,{method,headers:{'content-type':'application/json'},...(body===undefined?{}:{body:JSON.stringify(body)}),signal:AbortSignal.timeout(10000)});
  assert(response.ok,`${path}: ${response.status} ${await response.clone().text()}`);return response.status===204?null:response.json();
 };
 chat=await openChat(apiPort,owner);
 const {circle}=await chat.command('create_circle',{slug:'event-fixture',name:'Event fixture'});
 const {channel}=await chat.command('create_channel',{slug:'event-fixture-chat',name:'Event planning',kind:'private',circle_id:circle.id});
 await request(`/api/v1/circles/${circle.id}/features/heart-event-planning`,{enabled:true});
 await request(`/api/v1/channels/${channel.id}/process-bindings`,{process_key:'event-planning',namespace:'sproyt',definition_name:'sproyt-event-planning',definition_version:'1.0.0',enabled:true});
 for(const [name,metadata,pause] of [['original-title-only',{title:'Original event planning'},true],['client-spoof',{title:'Server correlation wins',process_link_id:'client-spoof'},false],['omitted-metadata',undefined,false]]){
  if(pause)await run('wslc',['stop','--time','10',heart]);
  const body={channel_id:channel.id,request_id:`${name}-start`,namespace:'sproyt',definition_name:'sproyt-event-planning',definition_version:'1.0.0',...(metadata===undefined?{}:{metadata})};
  const started=await request('/api/v1/processes',body),id=started.process_link_id;assert.match(id,/^[a-f0-9-]{36}$/);
  const replay=await request('/api/v1/processes',{...body,metadata:{process_link_id:'replay-spoof'}});assert.equal(replay.process_link_id,id);
  let chatPersisted=null,queued=null,pauseOutbox=null;
  if(pause){
   const ack=await chat.command('send_message',{channel_id:channel.id,body:'Chat persists while event START waits.'});
   const history=await chat.command('load_recent_messages',{channel_id:channel.id,after:0,limit:50});assert(history.messages.some(m=>m.id===ack.message.id));chatPersisted=true;
   queued=(await request(`/api/v1/processes/${id}`,undefined,'GET')).process.status;assert.equal(queued,'starting');
   pauseOutbox=await until(async()=>{const row=await pgJson('sproyt_fixture',`select json_build_object('attempts',attempts,'status',status,'lastError',last_error) from process_outbox where process_link_id='${id}' and operation='start'`);return row.status==='pending' && row.lastError?row:null;},'START was not durably rescheduled while Heart unavailable');
   await run('wslc',['start',heart]);await until(ready,'Heart restart failed');
  }
  const view=await until(async()=>{const v=await request(`/api/v1/processes/${id}`,undefined,'GET');return v.process.status==='active'?v:null;},'START did not recover');
  const iid=view.process.heart_instance_id;assert.match(iid,/^[a-f0-9-]{36}$/);
  const instance=()=>pgJson('heart_budget',`select row_to_json(x) from (select id,status,current_node,metadata from heart.instance where id='${iid}') x`);
  const waiting=await until(async()=>{const v=await instance();return v.current_node==='wait-for-decision'?v:null;},'real event workflow did not reach receive node');
  assert.equal(waiting.metadata.process_link_id,id);assert.equal(waiting.metadata.title,metadata?.title);
  const persisted=await pgJson('sproyt_fixture',`select json_build_object('rows',count(*),'metadata',min((payload->'command'->'metadata')::text)::json,'attempts',max(attempts),'status',min(status)) from process_outbox where process_link_id='${id}' and operation='start'`);
  assert.equal(persisted.rows,1);assert.equal(persisted.metadata.process_link_id,id);assert.equal(persisted.status,'completed');if(pause)assert(persisted.attempts>=2);
  const decision={request_id:`${name}-decision`,payload:{decision:'yes'}};
  const sent=await request(`/api/v1/processes/${id}/messages`,decision);
  assert.equal((await request(`/api/v1/processes/${id}/messages`,decision)).outbox_id,sent.outbox_id);
  const done=await until(async()=>{const v=await instance();return v.status==='completed'?v:null;},'real event workflow did not complete');
  assert.equal(done.current_node,'completed');assert.equal(done.metadata.process_link_id,id);assert.equal(done.metadata.decision,'yes');
  const final=await until(async()=>{const v=await request(`/api/v1/processes/${id}`,undefined,'GET');return v.events.some(e=>e.event_type==='process.correlated')?v:null;},'correlation outbox not completed');
  assert.equal(final.events.filter(e=>e.event_type==='process.started').length,1);assert.equal(final.events.filter(e=>e.event_type==='process.correlated').length,1);
  const keys=await pgJson('heart_budget',`select json_build_object('count',count(*),'instances',array_agg(instance_id)) from heart.instance_start_key where instance_id='${iid}'`);assert.equal(keys.count,1);
  const result={name,processLinkId:id,heartInstanceId:iid,chatPersisted,queuedWhileUnavailable:queued,pauseOutbox,startOutbox:persisted,waitingNode:waiting.current_node,completedInstance:done,startKeys:keys,startedEvents:1,correlatedEvents:1};
  evidence.scenarios.push(result);await save();console.log(`${name}: actual event workflow completed; one durable START and correlation`);
 }
}catch(error){evidence.errors.push(error.message);throw error;}
finally{
 if(chat)chat.close();await stopApi();
 const discovered=(await run('wslc',['list','--all','--filter','label='+label,'--no-trunc','--quiet'])).split(/\r?\n/).filter(id=>/^[a-f0-9]{64}$/.test(id));
 for(const id of [...new Set([...containers,...discovered])].toReversed()){
  const metadata=await inspect(id);assert.equal(metadata.Config.Labels[labelKey],nonce,'refusing cleanup of unowned container');
  if(metadata.State.Running)await run('wslc',['stop','--time','5',id]);await run('wslc',['remove','--volumes',id]);
 }
 if(network){assert.equal(JSON.parse(await run('wslc',['network','inspect',network]))[0].Labels[labelKey],nonce);await run('wslc',['network','remove',network]);}
 evidence.cleaned=true;await save();console.log(`Evidence: ${output}`);
}