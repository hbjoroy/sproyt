import {afterEach,it,expect,vi} from 'vitest';
import {bindReactionGesture} from '../src/reactions';
afterEach(()=>{vi.useRealTimers();document.body.replaceChildren();});
function pointer(el:HTMLElement,type:string,x=0,y=0){const event=new Event(type,{bubbles:true});Object.assign(event,{pointerType:'touch',clientX:x,clientY:y});el.dispatchEvent(event);}
it('opens after a deliberate hold, while movement and release cancel it',()=>{
 vi.useFakeTimers();const el=document.createElement('article');document.body.append(el);const open=vi.fn();const dispose=bindReactionGesture(el,open);
 pointer(el,'pointerdown');vi.advanceTimersByTime(499);expect(open).not.toHaveBeenCalled();vi.advanceTimersByTime(1);expect(open).toHaveBeenCalledWith(el);open.mockClear();
 pointer(el,'pointerdown');pointer(el,'pointermove',20);vi.advanceTimersByTime(600);expect(open).not.toHaveBeenCalled();
 pointer(el,'pointerdown');pointer(el,'pointerup');vi.advanceTimersByTime(600);expect(open).not.toHaveBeenCalled();dispose();
});
it('uses right click without taking over links or interactive touch controls',()=>{
 vi.useFakeTimers();const el=document.createElement('article');el.innerHTML='<a href="#">Link</a><button>Reply</button>';document.body.append(el);const open=vi.fn();const dispose=bindReactionGesture(el,open);
 const link=new MouseEvent('contextmenu',{bubbles:true,cancelable:true});el.querySelector('a')!.dispatchEvent(link);expect(link.defaultPrevented).toBe(false);
 pointer(el.querySelector('button')!,'pointerdown');vi.advanceTimersByTime(600);expect(open).not.toHaveBeenCalled();
 const context=new MouseEvent('contextmenu',{bubbles:true,cancelable:true});el.dispatchEvent(context);expect(context.defaultPrevented).toBe(true);expect(open).toHaveBeenCalledOnce();dispose();
});
