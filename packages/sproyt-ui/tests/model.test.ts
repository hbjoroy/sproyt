import {describe,it,expect,beforeEach} from 'vitest';
import {initialState,update,getState,select,draft,send,retry,createConversation,reply,threadDraft,sendThread,closeThread} from '../examples/shared/model';
beforeEach(()=>update(initialState()));
describe('demo conversation state',()=>{
 it('preserves independent drafts and does not leak messages between conversations',()=>{draft('Walk draft');select('next');draft('Sea draft');send('Sea message');expect(getState().messages.next.at(-1)?.body).toBe('Sea message');select('walk');expect(getState().drafts.walk).toBe('Walk draft');expect(getState().messages.walk.some(m=>m.body==='Sea message')).toBe(false);});
 it('retries a failed message without duplicating it',()=>{update({failNext:true});send('Hello');const before=getState().messages.walk;const message=before.at(-1)!;expect(message.status).toBe('failed');expect(getState().failNext).toBe(false);retry(message.id);expect(getState().messages.walk).toHaveLength(before.length);expect(getState().messages.walk.at(-1)?.status).toBe('sent');});
 it('rejects empty and duplicate conversation names',()=>{expect(createConversation(' ')).toBe(false);expect(createConversation('Dinner')).toBe(true);expect(createConversation(' dinner ')).toBe(false);expect(getState().conversations.at(-1)?.name).toBe('Dinner');});
 it('keeps thread replies out of the main timeline and preserves separate drafts',()=>{
  draft('Channel draft');reply(getState().messages.walk[0]);threadDraft('Thread draft');closeThread();
  expect(getState().drafts.walk).toBe('Channel draft');expect(getState().threadDrafts.a).toBe('Thread draft');
  reply(getState().messages.walk[0]);sendThread('A real threaded reply');
  expect(getState().messages.walk.filter(m=>!m.parentId)).toHaveLength(3);
  expect(getState().messages.walk.filter(m=>m.parentId==='a')).toHaveLength(1);
  expect(getState().messages.walk.at(-1)?.body).toBe('A real threaded reply');expect(getState().threadDrafts.a).toBe('');
  expect(getState().drafts.walk).toBe('Channel draft');
 });
 it('opens the root thread for a reply and closes it when changing conversations',()=>{
  reply(getState().messages.walk[0]);sendThread('Reply');const child=getState().messages.walk.at(-1)!;
  closeThread();reply(child);expect(getState().threadId).toBe('a');
  select('next');expect(getState().threadId).toBeNull();sendThread('Cannot create an orphan');
  expect(getState().messages.next).toHaveLength(1);
 });
});
