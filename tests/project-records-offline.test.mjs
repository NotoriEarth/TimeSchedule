import test, { after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { prepare as prepareRecord, sync as syncRecords, status, PARENT_PAGE_ID } from '../scripts/project-records.mjs';
import { gitFixture } from './helpers/record-git-fixture.mjs';
const fixtureDirectory = fs.mkdtempSync(path.join(os.tmpdir(), 'timeschedule-offline-git-'));
const fixture = gitFixture(fixtureDirectory);
after(() => fs.rmSync(fixtureDirectory, { recursive: true, force: true }));
const prepare = (input, options) => prepareRecord(input, { ...options, projectRoot: fixture.repo });
const sync = options => syncRecords({ spaceEnabled: true, vaultRoot: path.join(options.stateDir, 'vault'), ...options, projectRoot: fixture.repo });
const pageId='page_1234567890abcdef1234567890abcdef';
const saved=r=>({outcome:'saved',workId:r.workId,revision:r.revision,entryId:r.workId});
const published=j=>({pageId,pageUrl:`https://chatgpt.com/space/${pageId}`,parentPageId:PARENT_PAGE_ID,title:j.source.title,markdown:j.pageMarkdown,indexVerified:true});
function setup(t){const dir=fs.mkdtempSync(path.join(os.tmpdir(),'timeschedule-offline-'));t.after(()=>fs.rmSync(dir,{recursive:true,force:true}));return dir;}
function source(){return {workId:randomUUID(),projectTitle:'TimeSchedule',repoPath:fixture.repo,gitChanges:fixture.gitChanges,date:'2026-10-31',kind:'development',title:'연결 장애 기록',status:'verification_pending',detailedMarkdown:'원인 미확인. 테스트만 통과했고 실제 입력은 미확인.',diarySummary:'원인 미확인. 테스트만 통과했고 실제 입력은 미확인.'};}
test('offline work is in the diary and all accumulated work syncs after reconnection',async t=>{
 const stateDir=setup(t), a=source(),b=source(), records=[];
 await prepare(a,{stateDir});await prepare(b,{stateDir});
 let calls=0;const importer=async r=>{records.push(r);return saved(r);};
 const offline=async()=>{calls++;throw Object.assign(Error('not connected'),{publicationNotStarted:true});};
 await sync({stateDir,publisher:offline,importer});
 assert.equal(records.length,2);assert.ok(records.every(r=>r.deliveryState==='pending'&&r.detail===a.detailedMarkdown&&!r.spaceUrl));
 assert.ok(status({stateDir}).every(j=>j.phase==='waiting_space'));
 await sync({stateDir,publisher:offline,importer});assert.equal(calls,2,'backoff must skip model calls');
 await sync({stateDir,retryNow:true,publisher:async(j,o)=>{assert.equal(o.recoverOnly,false);return published(j);},importer});
 assert.ok(status({stateDir}).every(j=>j.phase==='complete'));assert.equal(records.filter(r=>r.deliveryState==='synced').length,2);
 assert.equal(records[2].revision,records[0].revision+1);
});
test('new revisions during an outage retain earlier intent and use one Space page in order',async t=>{
 const stateDir=setup(t),input=source();await prepare(input,{stateDir});
 await sync({stateDir,publisher:async()=>{throw Object.assign(Error('offline'),{publicationNotStarted:true});},importer:async r=>saved(r)});
 await prepare({...input,detailedMarkdown:input.detailedMarkdown+'\n\n후속 확인: 재현 조건을 찾았다.'},{stateDir});
 const revisions=[];await sync({stateDir,retryNow:true,publisher:async j=>{revisions.push(j.revision);if(j.revision===2)assert.equal(j.space.pageId,pageId);return published(j);},importer:async r=>saved(r)});
 assert.deepEqual(revisions,[1,2]);assert.ok(status({stateDir}).every(j=>j.phase==='complete'));
});
test('a failed old item does not block a different project work item',async t=>{
 const stateDir=setup(t),a=source(),b=source();await prepare(a,{stateDir});await prepare(b,{stateDir});
 await sync({stateDir,publisher:async j=>{if(j.source.workId===a.workId)throw Object.assign(Error('offline'),{publicationNotStarted:true});return published(j);},importer:async r=>saved(r)});
 assert.equal(status({stateDir,workId:b.workId})[0].phase,'complete');
});
test('a later local revision cannot prevent its earlier Space delivery from completing',async t=>{
 const stateDir=setup(t),input=source();let stored=0;
 const importer=async r=>{if(r.revision<stored)return {...saved(r),outcome:'stale'};stored=r.revision;return saved(r);};
 await prepare(input,{stateDir});await sync({stateDir,importer,publisher:async()=>{throw Object.assign(Error('offline'),{publicationNotStarted:true});}});
 await prepare({...input,detailedMarkdown:input.detailedMarkdown+'\n\n후속 작업을 로컬에 보관했다.'},{stateDir});
 await sync({stateDir,importer,publisher:async()=>{throw Error('backoff or prior work must block publication');}});
 assert.ok(stored>=3);
 await sync({stateDir,retryNow:true,importer,publisher:async j=>published(j)});
 assert.ok(status({stateDir}).every(j=>j.phase==='complete'));assert.equal(stored,4);
});
test('failed read-only recovery never clears an earlier uncertain publication',async t=>{
 const stateDir=setup(t);await prepare(source(),{stateDir});const importer=async r=>saved(r);
 await sync({stateDir,importer,publisher:async()=>{throw Error('response lost after create');}});
 await sync({stateDir,retryNow:true,importer,publisher:async(j,o)=>{assert.equal(o.recoverOnly,true);throw Object.assign(Error('authentication unavailable during recovery'),{publicationNotStarted:true});}});
 await sync({stateDir,retryNow:true,importer,publisher:async(j,o)=>{assert.equal(o.recoverOnly,true);return published(j);}});
 assert.equal(status({stateDir})[0].phase,'complete');
});
test('human Space conflicts remain terminal even when the failed attempt made no write',async t=>{
 const stateDir=setup(t);await prepare(source(),{stateDir});let calls=0;
 const publisher=async()=>{calls++;throw Object.assign(Error('user edited Space'),{conflict:true,publicationNotStarted:true});};
 await sync({stateDir,publisher,importer:async r=>saved(r)});
 assert.equal(status({stateDir})[0].phase,'conflict');
 await sync({stateDir,retryNow:true,publisher,importer:async r=>saved(r)});assert.equal(calls,1);
});
