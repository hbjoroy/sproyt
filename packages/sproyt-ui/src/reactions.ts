export const reactionEmoji = [
 ['👍','Tommel opp, ja, bra'],['❤️','Hjarte, glad i, love'],['😂','Latter, morsomt'],['🎉','Feiring, hurra'],['😮','Overraska, wow'],['🙏','Takk, please'],
 ['😊','Smil, glad'],['😍','Forelska'],['🤔','Tenker, spørsmål'],['😢','Trist'],['😎','Kul'],['🔥','Eld, fire'],['👏','Applaus'],['💯','Hundre, perfekt'],['✅','Ferdig, yes'],['👀','Ser, eyes'],['☕','Kaffi, coffee'],['🍰','Kake'],['🌊','Sjø, bølgje'],['🚶','Tur, gå'],['🌞','Sol'],['💪','Sterk'],['🤗','Klem'],['👋','Hei, ha det']
] as const;
export interface ReactionPickerOptions {selected?:string;onSelect:(emoji:string)=>void;title?:string;searchLabel?:string;moreLabel?:string;lessLabel?:string;closeLabel?:string;emptyLabel?:string;items?:ReadonlyArray<readonly [string,string]>}
/** A shared native-dialog widget for React, Vue and other DOM applications. */
export function openReactionPicker(anchor:HTMLElement,options:ReactionPickerOptions){
 const theme=anchor.closest('.sp-theme');if(!theme)return()=>{};
 const previous=document.activeElement as HTMLElement|null;
 const dialog=document.createElement('dialog');dialog.className='sp-reaction-picker';dialog.setAttribute('aria-label',options.title||'Reager på meldinga');
 const head=document.createElement('header');head.className='sp-reaction-head';
 const title=document.createElement('span');title.className='sp-kicker';title.textContent=options.title||'Reager på meldinga';
 const close=document.createElement('button');close.type='button';close.className='sp-button';close.textContent='×';close.setAttribute('aria-label',options.closeLabel||'Lukk');
 head.append(title,close);
 const grid=document.createElement('div');grid.className='sp-emoji-grid';grid.setAttribute('role','group');grid.setAttribute('aria-label',options.title||'Vel reaksjon');
 const search=document.createElement('input');search.type='search';search.className='sp-input';search.placeholder=options.searchLabel||'Finn emoji …';search.setAttribute('aria-label',options.searchLabel||'Finn emoji');search.hidden=true;
 const more=document.createElement('button');more.type='button';more.className='sp-reaction-more';more.textContent=options.moreLabel||'Fleire emoji';more.setAttribute('aria-expanded','false');
 const empty=document.createElement('p');empty.className='sp-help';empty.textContent=options.emptyLabel||'Ingen treff. Prøv eit anna ord.';empty.hidden=true;
 let expanded=false;const items=options.items||reactionEmoji;
 const position=()=>{const r=anchor.getBoundingClientRect();const box=dialog.getBoundingClientRect();const viewport=window.visualViewport;const w=viewport?.width||innerWidth,h=viewport?.height||innerHeight;const x=viewport?.offsetLeft||0,y=viewport?.offsetTop||0;dialog.style.left=`${Math.max(x+12,Math.min(r.left,x+w-box.width-12))}px`;dialog.style.top=`${Math.max(y+12,Math.min(r.top-box.height-8>=y+12?r.top-box.height-8:r.bottom+8,y+h-box.height-12))}px`;};
 const finish=()=>{dialog.close();dialog.remove();window.removeEventListener('resize',position);window.visualViewport?.removeEventListener('resize',position);if(previous?.isConnected&&previous!==document.body)previous.focus();else anchor.querySelector<HTMLButtonElement>('button')?.focus();};
 const draw=()=>{grid.replaceChildren();const q=search.value.toLocaleLowerCase();const choices=(expanded?items:items.slice(0,6)).filter(([emoji,label])=>!q||(emoji+' '+label).toLocaleLowerCase().includes(q));for(const [emoji,label] of choices){const button=document.createElement('button');button.type='button';button.className='sp-emoji';button.textContent=emoji;button.setAttribute('aria-label',label);button.title=label;button.setAttribute('aria-pressed',String(options.selected===emoji));button.onclick=()=>{options.onSelect(emoji);finish();};grid.append(button);}empty.hidden=choices.length>0;position();};
 more.onclick=()=>{expanded=!expanded;if(!expanded)search.value='';search.hidden=!expanded;more.textContent=expanded?(options.lessLabel||'Færre emoji'):options.moreLabel||'Fleire emoji';more.setAttribute('aria-expanded',String(expanded));draw();if(expanded)search.focus();};
 search.oninput=draw;close.onclick=finish;dialog.addEventListener('cancel',e=>{e.preventDefault();finish();});dialog.addEventListener('click',e=>{if(e.target===dialog){const r=dialog.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)finish();}});
 grid.addEventListener('keydown',e=>{const buttons=Array.from(grid.querySelectorAll('button'));const index=buttons.indexOf(document.activeElement as HTMLButtonElement);const shifts:Record<string,number>={ArrowRight:1,ArrowLeft:-1,ArrowDown:6,ArrowUp:-6};if(e.key in shifts&&buttons.length){e.preventDefault();buttons[(index+shifts[e.key]+buttons.length)%buttons.length].focus();}});
 dialog.append(head,search,grid,empty,more);theme.append(dialog);draw();dialog.showModal();position();grid.querySelector('button')?.focus();window.addEventListener('resize',position);window.visualViewport?.addEventListener('resize',position);return finish;
}
/** Long press cancels on movement, so ordinary touch scrolling still works. */
export function bindReactionGesture(element:HTMLElement,open:(anchor:HTMLElement)=>void){
 let timer:ReturnType<typeof setTimeout>|undefined;let x=0,y=0;let suppress=false;
 const cancel=()=>{clearTimeout(timer);timer=undefined;};
 const down=(e:PointerEvent)=>{suppress=false;if(e.pointerType!=='touch'||(e.target as Element).closest('button,a,input,textarea,select'))return;x=e.clientX;y=e.clientY;timer=setTimeout(()=>{suppress=true;open(element);},500);};
 const move=(e:PointerEvent)=>{if(Math.hypot(e.clientX-x,e.clientY-y)>10)cancel();};
 const context=(e:MouseEvent)=>{if((e.target as Element).closest('a,input,textarea,select'))return;e.preventDefault();cancel();if(!suppress)open(element);suppress=true;};
 const click=(e:MouseEvent)=>{if(suppress){e.preventDefault();e.stopPropagation();suppress=false;}};
 element.addEventListener('pointerdown',down);element.addEventListener('pointermove',move);element.addEventListener('pointerup',cancel);element.addEventListener('pointercancel',cancel);element.addEventListener('contextmenu',context);element.addEventListener('click',click,true);
 return()=>{cancel();element.removeEventListener('pointerdown',down);element.removeEventListener('pointermove',move);element.removeEventListener('pointerup',cancel);element.removeEventListener('pointercancel',cancel);element.removeEventListener('contextmenu',context);element.removeEventListener('click',click,true);};
}
