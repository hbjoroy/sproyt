import {afterEach,describe,it,expect,vi} from 'vitest';
import {render,fireEvent,cleanup as cleanupReact} from '@testing-library/react';
import {render as renderVue,cleanup as cleanupVue} from '@testing-library/vue';
import {Theme,Composer,TextField,EventCard,Button,Message} from '../src/react/index';
import {h} from 'vue';
import * as Vue from '../src/vue/index';
afterEach(()=>{cleanupReact();cleanupVue();});
describe('React controls',()=>{
 it('keeps string authors compatible and rich author actions independent',()=>{
  const badge=vi.fn(),status=vi.fn();
  const r=render(<Message author="Anne" time="12:00">Hello</Message>);
  expect(r.container.querySelector('.sp-message-meta strong')?.textContent).toBe('Anne');
  r.rerender(<Message author="Anne" time="12:00" authorContent={<><strong>Anne</strong><button onClick={badge}>Badge</button><button onClick={status}>Edit status</button></>}>Hello</Message>);
  fireEvent.click(r.getByRole('button',{name:'Edit status'}));
  expect(status).toHaveBeenCalledTimes(1);expect(badge).not.toHaveBeenCalled();
  fireEvent.click(r.getByRole('button',{name:'Badge'}));expect(badge).toHaveBeenCalledTimes(1);
 });
 it('keeps a draft controlled, trims submitted text and ignores IME Enter',()=>{const send=vi.fn();const change=vi.fn();const r=render(<Composer label="Message" value="  Hello  " onChange={change} onSend={send}/>);const input=r.getByLabelText('Message');fireEvent.keyDown(input,{key:'Enter',ctrlKey:true,isComposing:true});expect(send).not.toHaveBeenCalled();fireEvent.keyDown(input,{key:'Enter',ctrlKey:true});expect(send).toHaveBeenCalledExactlyOnceWith('Hello');expect((input as HTMLTextAreaElement).value).toBe('  Hello  ');});
 it('does not submit an empty or busy composer',()=>{const send=vi.fn();const r=render(<Composer label="Message" value=" " onChange={()=>{}} onSend={send}/>);fireEvent.submit(r.container.querySelector('form')!);expect(send).not.toHaveBeenCalled();r.rerender(<Composer label="Message" value="Hello" busy onChange={()=>{}} onSend={send}/>);fireEvent.submit(r.container.querySelector('form')!);expect(send).not.toHaveBeenCalled();});
 it('uses unique field IDs and connects validation text',()=>{const r=render(<><TextField label="First" error="Required"/><TextField label="Second"/></>);const first=r.getByLabelText('First');expect(first.id).not.toBe(r.getByLabelText('Second').id);expect(document.getElementById(first.getAttribute('aria-describedby')!)?.textContent).toBe('Required');});
 it('emits a single controlled RSVP selection',()=>{const change=vi.fn();const r=render(<EventCard title="Dinner" when="18:00" value="yes" onChange={change}/>);expect((r.getByRole('radio',{name:'Eg kjem'}) as HTMLInputElement).checked).toBe(true);fireEvent.click(r.getByRole('radio',{name:'Kanskje'}));expect(change).toHaveBeenCalledWith('maybe');});
 it('preserves content and defaults buttons to non-submit',()=>{const r=render(<Theme mode="dark"><Button>Save</Button></Theme>);expect(r.getByRole('button').getAttribute('type')).toBe('button');expect(r.container.querySelector('[data-theme=dark]')).toBeTruthy();});
});
describe('Vue controls',()=>{
 it('supports the same author slot while retaining plain author rendering',async()=>{
  const plain=renderVue(Vue.Message,{props:{author:'Anne',time:'12:00'},slots:{default:'Hello'}});
  expect(plain.container.querySelector('.sp-message-meta strong')?.textContent).toBe('Anne');plain.unmount();
  const badge=vi.fn(),status=vi.fn();
  const r=renderVue(Vue.Message,{props:{author:'Anne',time:'12:00'},slots:{default:'Hello',author:()=>[h('strong','Anne'),h('button',{onClick:badge},'Badge'),h('button',{onClick:status},'Edit status')]}});
  await fireEvent.click(r.getByRole('button',{name:'Edit status'}));expect(status).toHaveBeenCalledTimes(1);expect(badge).not.toHaveBeenCalled();
  await fireEvent.click(r.getByRole('button',{name:'Badge'}));expect(badge).toHaveBeenCalledTimes(1);
 });
 it('renders Theme content and emits a trimmed composer send',async()=>{const r=renderVue(Vue.Composer,{props:{label:'Message',modelValue:'  Hello  '}});await fireEvent.submit(r.container.querySelector('form')!);expect(r.emitted().send).toEqual([['Hello']]);expect((r.getByLabelText('Message') as HTMLTextAreaElement).value).toBe('  Hello  ');const theme=renderVue(Vue.Theme,{props:{mode:'dark'},slots:{default:'Theme content'}});expect(theme.getByText('Theme content')).toBeTruthy();});
 it('forwards field attributes and emits model updates',async()=>{const r=renderVue(Vue.TextField,{props:{label:'Name',modelValue:'',error:'Required'},attrs:{autocomplete:'name'}});const input=r.getByLabelText('Name');expect(input.getAttribute('autocomplete')).toBe('name');await fireEvent.input(input,{target:{value:'Anne'}});expect(r.emitted()['update:modelValue']).toEqual([['Anne']]);expect(document.getElementById(input.getAttribute('aria-describedby')!)?.textContent).toBe('Required');});
 it('blocks busy sends and emits RSVP changes',async()=>{const r=renderVue(Vue.Composer,{props:{label:'Message',modelValue:'Hello',busy:true}});await fireEvent.submit(r.container.querySelector('form')!);expect(r.emitted().send).toBeUndefined();const e=renderVue(Vue.EventCard,{props:{title:'Dinner',when:'18:00',modelValue:'yes'}});await fireEvent.click(e.getByRole('radio',{name:'Kanskje'}));expect(e.emitted()['update:modelValue']).toEqual([['maybe']]);});
});
