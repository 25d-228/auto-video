(function(){
  var COVERS = window.__COVERS__||{}, ITEMS = window.__ITEMS__||[], IDENT = window.__IDENT__||{};
  var GRPCOVER={}, GRPAR={}; for(var _k in IDENT){ var _v=IDENT[_k]; if(_v&&_v.cat==='vrc'&&_v.cover&&_v.title){ GRPCOVER[_v.title]=_v.cover; GRPAR[_v.title]=_v.ar||0.72; } }
  // provider keys + fetched-metadata cache (entered by the user; the real app's method)
  var KEYS={}; try{KEYS=JSON.parse(localStorage.getItem('av_keys')||'{}');}catch(e){}
  var META={}; try{META=JSON.parse(localStorage.getItem('av_meta')||'{}');}catch(e){}
  for(var _mk in META){ if(IDENT[_mk]) for(var _f in META[_mk]) IDENT[_mk][_f]=META[_mk][_f]; }
  function saveKeys(){ try{localStorage.setItem('av_keys',JSON.stringify(KEYS));}catch(e){} }
  function saveMeta(){ try{localStorage.setItem('av_meta',JSON.stringify(META));}catch(e){} }
  var av_paths={}; try{av_paths=JSON.parse(localStorage.getItem('av_paths')||'{}');}catch(e){}
  function savePaths(){ try{localStorage.setItem('av_paths',JSON.stringify(av_paths));}catch(e){} }
  function saveHelperPath(cat,p){ if(!window.__PROXY__)return; try{ fetch(window.__PROXY__+'/savepath?cat='+encodeURIComponent(cat)+'&path='+encodeURIComponent(p||'')); }catch(e){} }
  // provider keys persist to disk via the helper too (once saved, saved — survives relaunch)
  function saveHelperKey(prov,v){ if(!window.__PROXY__)return; try{ fetch(window.__PROXY__+'/savekey?provider='+encodeURIComponent(prov)+'&key='+encodeURIComponent(v||'')); }catch(e){} }
  function loadHelperKeys(cb){ if(!window.__PROXY__){ if(cb)cb(); return; } fetch(window.__PROXY__+'/keys').then(function(r){return r.json();}).then(function(d){ if(d&&typeof d==='object'){ for(var k in d){ if(d[k]&&!KEYS[k]) KEYS[k]=d[k]; } saveKeys(); } if(cb)cb(); }).catch(function(){ if(cb)cb(); }); }
  function loadHelperPaths(cb){ if(!window.__PROXY__){cb&&cb();return;} fetch(window.__PROXY__+'/paths').then(function(r){return r.json();}).then(function(d){ if(d&&typeof d==='object'){ for(var k in d){ if(d[k]) av_paths[k]=d[k]; } savePaths(); } cb&&cb(); }).catch(function(){cb&&cb();}); }
  function getBase(cat){ var el=document.getElementById('path-'+cat); var v=el?el.value:''; return (v||av_paths[cat]||'').trim(); }
  function isAbs(p){ return /^[A-Za-z]:[\\\/]/.test(p); }
  function jsonp(url){ return new Promise(function(res,rej){ var cb='__jp'+(jsonp._i=(jsonp._i||0)+1); var s=document.createElement('script');
    window[cb]=function(d){res(d);try{delete window[cb];}catch(e){} s.remove();}; s.onerror=function(){rej();try{delete window[cb];}catch(e){} s.remove();};
    s.src=url+(url.indexOf('?')<0?'?':'&')+'callback='+cb; document.body.appendChild(s);
    setTimeout(function(){ if(window[cb]){rej(); try{delete window[cb];}catch(e){} try{s.remove();}catch(e){}} },12000); }); }
  function cidOf(cover){ var m=(cover||'').match(/\/digital\/video\/([^\/]+)\//); return m?m[1]:''; }
  function toast(msg){ var app=document.querySelector('.mk-app'); if(!app)return; var t=document.createElement('div'); t.className='mk-toast'; t.textContent=msg; app.appendChild(t); setTimeout(function(){t.classList.add('show');},10); setTimeout(function(){t.classList.remove('show'); setTimeout(function(){try{t.remove();}catch(e){}},250);},1700); }
  function fullPath(cat,fn){ return getBase(cat).replace(/[\\\/]+$/,'')+'\\'+fn; }
  function itemPath(it){ return it.path || fullPath(it.cat, it.fname||''); }   // real scanned path wins (handles subfolders)
  function playFile(id){ var it=findItem(id); if(!it)return; var base=getBase(it.cat), fn=it.fname||'', p=itemPath(it);
    if(window.__PROXY__ && (it.path || (isAbs(base) && fn))){
      fetch(window.__PROXY__+'/open?path='+encodeURIComponent(p)).then(function(r){return r.json();})
        .then(function(d){ toast(d&&d.ok?'▶ Opening in your default player…':'Not found — check the folder path in Settings'); })
        .catch(function(){ toast('Local helper not reachable'); });
      return;
    }
    var hd=HANDLES[fn];
    if(hd){ hd.getFile().then(function(f){ var url=URL.createObjectURL(f);
      var ov=document.getElementById('mk-player'); if(!ov){ ov=document.createElement('div'); ov.id='mk-player'; ov.className='mk-playov'; document.querySelector('.mk-app').appendChild(ov); }
      ov.innerHTML='<div class="mk-playbox"><div class="mk-playbar"><span class="mk-playtitle">'+esc(fn)+'</span><span class="mk-playclose" id="mk-playclose">✕</span></div><video src="'+url+'" controls autoplay></video></div>';
      ov.classList.add('show');
      function close(){ ov.classList.remove('show'); var v=ov.querySelector('video'); if(v){try{v.pause();}catch(e){}} setTimeout(function(){ try{URL.revokeObjectURL(url);}catch(e){} ov.innerHTML=''; },200); }
      var cb=document.getElementById('mk-playclose'); if(cb) cb.onclick=close; ov.onclick=function(e){ if(e.target===ov) close(); };
    }).catch(function(){ toast('Could not open the file'); }); return; }
    toast('Set this category’s full folder path in Settings → Library to open in your player');
  }
  function revealFile(id){ var it=findItem(id); if(!it)return; var base=getBase(it.cat), p=itemPath(it);
    if(window.__PROXY__ && (it.path || isAbs(base))){
      fetch(window.__PROXY__+'/reveal?path='+encodeURIComponent(p)).then(function(r){return r.json();})
        .then(function(d){ toast(d&&d.ok?'Opening in Explorer…':'Not found — check the folder path in Settings'); })
        .catch(function(){ toast('Local helper not reachable'); });
      return;
    }
    toast('Set this category’s full folder path in Settings → Library to reveal in Explorer');
  }
  // ----- right-click context menu (Library) -----
  function closeCtx(){ var m=document.getElementById('mk-ctxmenu'); if(m){ try{m.remove();}catch(e){} } }
  function showCtx(x,y,items){ closeCtx();
    var m=document.createElement('div'); m.id='mk-ctxmenu'; m.className='mk-ctxmenu';
    items.forEach(function(o){
      if(o.sep){ var s=document.createElement('div'); s.className='mk-ctxsep'; m.appendChild(s); return; }
      var b=document.createElement('div'); b.className='mk-ctxitem'+(o.danger?' danger':''); b.innerHTML=o.label;
      b.onclick=function(e){ e.stopPropagation(); closeCtx(); if(o.onClick) o.onClick(); };
      m.appendChild(b);
    });
    (document.querySelector('.mk-app')||document.body).appendChild(m);
    var r=m.getBoundingClientRect(), vw=window.innerWidth, vh=window.innerHeight;
    m.style.left=Math.max(6,Math.min(x, vw-r.width-8))+'px';
    m.style.top=Math.max(6,Math.min(y, vh-r.height-8))+'px';
  }
  document.addEventListener('click',closeCtx);
  window.addEventListener('blur',closeCtx);
  // which on-disk files a card stands for: a grouped VR work also sweeps its hidden part files
  function targetFnames(it){ var info=it.user?IDENT[it.fname]:null; var fnames=[it.fname];
    if(groupParts && info && info.parts){ var code=info.title;
      for(var fn in IDENT){ if(IDENT[fn]&&IDENT[fn].grp===code&&fnames.indexOf(fn)<0) fnames.push(fn); } }
    return fnames; }
  function deleteItem(id){ var it=findItem(id); if(!it)return;
    var base=getBase(it.cat);
    if(!(window.__PROXY__ && (it.path || isAbs(base)))){ toast('Set this category’s full folder path in Settings → Library to delete'); return; }
    var fnames=targetFnames(it), label=disp(it).title||it.fname, n=fnames.length;
    if(!confirm('Move “'+label+'”'+(n>1?(' and its '+n+' part files'):'')+' to the Recycle Bin?')) return;
    var paths=(it.path && fnames.length===1) ? [it.path] : fnames.map(function(fn){ return fullPath(it.cat,fn); });
    Promise.all(paths.map(function(p){ return fetch(window.__PROXY__+'/delete?path='+encodeURIComponent(p)).then(function(r){return r.json();}).catch(function(){return {ok:false};}); }))
      .then(function(res){ var okc=res.filter(function(x){return x&&x.ok;}).length;
        if(okc>0){ var u=userLib[it.cat]; if(u&&u.files){ u.files=u.files.filter(function(x){return fnames.indexOf(x.fname)<0;}); u.count=u.files.length; saveUL(); }
          fnames.forEach(function(fn){ if(IDENT[fn]) IDENT[fn].hide=true; });
          toast(okc+' file'+(okc>1?'s':'')+' moved to Recycle Bin'); libFill();
        } else { toast('Delete failed — '+((res[0]&&res[0].err)||'check the folder path')); } }); }
  async function scanFolder(cat){
    var el=document.getElementById('path-'+cat); var base=el?el.value.trim():getBase(cat);
    if(!isAbs(base)){ toast('Type the full folder path first, e.g. D:\\Movies'); return; }
    if(!window.__PROXY__){ toast('Local helper not running'); return; }
    av_paths[cat]=base; savePaths();
    toast('Scanning '+base+'…');
    try{ await fetch(window.__PROXY__+'/savepath?cat='+encodeURIComponent(cat)+'&path='+encodeURIComponent(base)); }catch(e){}
    lib.cat=cat; lib.page=1; var lr=document.getElementById('mkv-lib'); if(lr) lr.checked=true; renderLib();
    loadLibrary();   // recursive scan of all configured folders via the sidecar (real full paths + lazy covers)
  }
  async function proxyFill(it,info){
    if(!window.__PROXY__) return false;
    var cid=cidOf(info.cover), code=info.title||it.title;
    try{
      var d=await fetch(window.__PROXY__+'/meta?cid='+encodeURIComponent(cid)+'&code='+encodeURIComponent(code)+'&cat='+it.cat).then(function(r){return r.json();});
      if(!d) return false; var rec={};
      ['jatitle','cast_ja','cast','date','runtime'].forEach(function(f){ if(d[f]) rec[f]=d[f]; });
      if(!Object.keys(rec).length) return false;
      for(var f in rec) info[f]=rec[f]; META[it.fname]=Object.assign(META[it.fname]||{},rec); saveMeta(); return true;
    }catch(e){ return false; }
  }
  async function dmmFill(it,info){
    if(!KEYS.dmmApi||!KEYS.dmmAff) return false;
    var cid=cidOf(info.cover); if(!cid) return false;
    var url='https://api.dmm.com/affiliate/v3/ItemList?api_id='+encodeURIComponent(KEYS.dmmApi)+'&affiliate_id='+encodeURIComponent(KEYS.dmmAff)+'&site=FANZA&service=digital&floor=videoa&cid='+encodeURIComponent(cid)+'&hits=1&output=json';
    try{ var d=await jsonp(url); var item=d&&d.result&&d.result.items&&d.result.items[0]; if(!item) return false;
      var rec={}; if(item.title) rec.jatitle=item.title; if(item.date) rec.date=(''+item.date).slice(0,10);
      if(item.volume) rec.runtime=(''+item.volume).replace(/[^0-9]/g,'')+' min';
      var a=(item.iteminfo&&item.iteminfo.actress)||[]; if(a.length) rec.cast_ja=a.map(function(x){return x.name;}).join(', ');
      for(var f in rec) info[f]=rec[f]; META[it.fname]=Object.assign(META[it.fname]||{},rec); saveMeta(); return true;
    }catch(e){ return false; } }
  async function tmdbFill(it,info){
    if(!KEYS.tmdb) return false;
    var title=info.title||it.title, year=(info.date||'').slice(0,4);
    try{ var s=await fetch('https://api.themoviedb.org/3/search/movie?api_key='+encodeURIComponent(KEYS.tmdb)+'&query='+encodeURIComponent(title)+(year?'&year='+year:'')).then(function(r){return r.json();});
      var m=s&&s.results&&s.results[0]; if(!m) return false;
      var det=await fetch('https://api.themoviedb.org/3/movie/'+m.id+'?api_key='+encodeURIComponent(KEYS.tmdb)).then(function(r){return r.json();});
      var rec={}; if(det.genres) rec.genre=det.genres.slice(0,3).map(function(g){return g.name;}).join(', ');
      if(det.runtime) rec.runtime=det.runtime+' min'; if(det.release_date) rec.date=det.release_date;
      for(var f in rec) info[f]=rec[f]; META[it.fname]=Object.assign(META[it.fname]||{},rec); saveMeta(); return true;
    }catch(e){ return false; } }
  function cov(u){ return COVERS[u]||''; }
  // resilient cover loading: if the proxied /img fails, retry the raw source URL, then fall to a gradient
  window.mkImgErr=function(el){
    var r=el.getAttribute('data-raw');
    if(r){ el.removeAttribute('data-raw'); el.src=r; return; }
    el.onerror=null; el.style.display='none';
    var p=el.parentNode, t=el.getAttribute('data-t')||'';
    if(p){ var x=h(t); p.style.background='linear-gradient(160deg,hsl('+(x%360)+',34%,42%),hsl('+((x+40)%360)+',34%,26%))';
      if(t && !p.querySelector('.mk-noartt')){ var s=document.createElement('span'); s.className='mk-noartt'; s.style.position='absolute'; s.style.padding='6px'; s.style.textAlign='center'; s.textContent=t; p.appendChild(s); } }
  };
  function imgTag(c,title){
    var raw=(c&&c.indexOf('/img?u=')>=0)?decodeURIComponent(c.split('u=')[1].split('&')[0]):'';
    return '<img loading="lazy" src="'+c+'"'+(raw?' data-raw="'+esc(raw)+'"':'')+' data-t="'+esc(title||'')+'" onerror="mkImgErr(this)" alt="">';
  }
  function esc(s){return (''+(s||'')).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');}
  function fseed(n){return n>=1000?(n/1000).toFixed(1)+'k':''+n;}
  function relAdded(d){ if(d<=0)return 'just now'; if(d===1)return '1 day ago'; if(d<30)return d+' days ago'; return Math.round(d/30)+' mo ago';}
  function h(s){var n=0;for(var i=0;i<s.length;i++){n=(n*31+s.charCodeAt(i))>>>0;}return n;}
  var CATS=[['mov','Movies'],['tv','TV'],['ad','Adult'],['vrc','VR']];
  var DEST={mov:'D:\\Movies',tv:'E:\\TV',ad:'F:\\Adult',vrc:'G:\\VR'};
  function newestCat(c){return c==='ad'||c==='vrc';}

  // ---- real folder import (File System Access API; Chrome/Edge on localhost) ----
  var VIDEXT=/\.(mp4|mkv|avi|wmv|mov|m4v|ts|flv|webm|mpg|mpeg)$/i;
  var userLib={}; try{ userLib=JSON.parse(localStorage.getItem('av_userlib')||'{}'); }catch(e){ userLib={}; }
  var HANDLES={}; // fname -> FileSystemFileHandle (in-memory, this session) for real playback
  function saveUL(){ try{ localStorage.setItem('av_userlib',JSON.stringify(userLib)); }catch(e){} }
  function parseName(name,cat){
    var clean=name.replace(/\.[^.]+$/,'').replace(/[._]+/g,' ').replace(/\s+/g,' ').trim();
    if(cat==='ad'||cat==='vrc'){
      var m=clean.match(/([A-Za-z]{2,6})[-\s]?(\d{2,5})/);
      return {title:(m?(m[1].toUpperCase()+'-'+m[2]):clean.slice(0,24)), sub:(cat==='vrc'?'VR · ':'')+'imported'};
    }
    var ym=clean.match(/(19|20)\d{2}/), ep=clean.match(/S(\d{1,2})E(\d{1,2})/i);
    var title=clean.replace(/(19|20)\d{2}[\s\S]*$/,'').replace(/S\d{1,2}E\d{1,2}[\s\S]*$/i,'').trim()||clean;
    return {title:title, sub:(ep?('S'+parseInt(ep[1],10)+'E'+parseInt(ep[2],10)):(ym?ym[0]:'imported'))};
  }
  async function pickFolder(cat){
    if(!window.showDirectoryPicker){ alert('Picking a real folder needs Chrome or Edge (File System Access API).\nIn the finished app this is a native folder dialog on Windows & macOS.'); return; }
    var dir; try{ dir=await window.showDirectoryPicker(); }catch(e){ return; }
    toast('Scanning '+dir.name+'…');
    var files=[];
    async function walk(handle,depth){ if(depth>4)return;
      for await (var ent of handle.values()){
        if(ent.kind==='file'){ if(VIDEXT.test(ent.name)){ HANDLES[ent.name]=ent; var p=parseName(ent.name,cat); files.push({id:'u_'+cat+'_'+files.length,cat:cat,title:p.title,sub:p.sub,state:'own',user:true,fname:ent.name,runtime:0}); } }
        else if(ent.kind==='directory'){ try{ await walk(ent,depth+1);}catch(e){} }
      }
    }
    try{ await walk(dir,0);}catch(e){}
    files.sort(function(a,b){return a.title.localeCompare(b.title);});
    userLib[cat]={name:dir.name,count:files.length,files:files}; saveUL();
    toast(dir.name+' · '+files.length+' videos imported — set the full path in Settings for Play/Reveal');
    // send the filenames back to the agent via the confer server's /ask endpoint
    try{ fetch('/ask',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({ask:'import',cat:cat,folder:dir.name,files:files.map(function(x){return x.fname;})})}); }catch(e){}
    lib.cat=cat; lib.page=1;
    var lr=document.getElementById('mkv-lib'); if(lr) lr.checked=true;
    setTimeout(renderLib,0);
  }
  function findItem(id){
    var a=ITEMS.filter(function(x){return x.id===id;})[0]; if(a)return a;
    for(var k in userLib){ if(userLib[k]&&userLib[k].files){ var f=userLib[k].files.filter(function(x){return x.id===id;})[0]; if(f)return f; } }
    return null;
  }

  // ---- pagination: greedily fit as many cards as the page holds, no scrollbar
  //      (count per page varies because covers have a fixed height but natural width) ----
  // Numbered jump-to-page pager (javdb-style). Pages hold 2 rows of natural-width covers,
  // so the per-page count varies — measure the whole pool once to find page boundaries.
  function pgNav(pager, p, total, items, go){
    if(total<=1){ pager.style.display='none'; pager.innerHTML=''; return; }
    pager.style.display='flex';
    function numBtn(n){ return n===p ? '<span class="mk-pgcur">'+n+'</span>'
      : '<button class="mk-pgbtn mk-pgn" data-go="'+n+'">'+n+'</button>'; }
    var win=2, lo=Math.max(1,p-win), hi=Math.min(total,p+win), mid=[];
    if(lo>1){ mid.push(numBtn(1)); if(lo>2) mid.push('<span class="mk-pggap">…</span>'); }
    for(var i=lo;i<=hi;i++) mid.push(numBtn(i));
    if(hi<total){ if(hi<total-1) mid.push('<span class="mk-pggap">…</span>'); mid.push(numBtn(total)); }
    pager.innerHTML=
      '<div class="mk-pgside mk-pgleft"><button class="mk-pgbtn" data-go="'+(p-1)+'"'+(p<=1?' disabled':'')+'>‹</button></div>'+
      '<div class="mk-pgmid">'+mid.join('')+'</div>'+
      '<div class="mk-pgside mk-pgright"><span class="mk-pgnum">'+items+' items</span>'+
        '<button class="mk-pgbtn" data-go="'+(p+1)+'"'+(p>=total?' disabled':'')+'>›</button></div>';
    pager.querySelectorAll('[data-go]').forEach(function(b){ b.onclick=function(){ if(b.hasAttribute('disabled'))return; var t=parseInt(b.getAttribute('data-go'),10); if(t>=1&&t<=total) go(t); }; });
  }
  function paginate(grid, pager, pool, state, makeCard, onpage){
    var sig=pool.length+'|'+((pool[0]&&pool[0].id)||'')+'|'+((pool[pool.length-1]&&pool[pool.length-1].id)||'')+'|'+grid.clientWidth+'x'+grid.clientHeight;
    if(state._pgSig!==sig){                          // (re)compute page boundaries: pack as many rows as fit the window
      grid.innerHTML='';
      for(var k=0;k<pool.length;k++) grid.insertAdjacentHTML('beforeend', makeCard(pool[k]));
      var cards=grid.children, lastTop=-1, rowStart=[], rowTops=[];
      for(var c=0;c<cards.length;c++){ var t=cards[c].offsetTop; if(t>lastTop+1){ rowStart.push(c); rowTops.push(t); lastTop=t; } }
      var rowH=rowTops.length>1?(rowTops[1]-rowTops[0]):((cards[0]?cards[0].offsetHeight:200)+16);
      var avail=grid.clientHeight||400;
      var cap=Math.max(1, Math.floor((avail-(rowTops[0]||0))/rowH + 0.02));   // rows that fit the window
      var ps=[]; for(var r=0;r<rowStart.length;r+=cap) ps.push(rowStart[r]);
      state._pgSig=sig; state._pageStarts=ps.length?ps:[0];
    }
    var pages=state._pageStarts, total=pages.length;
    if(state.page>total) state.page=total; if(state.page<1) state.page=1;
    var s=pages[state.page-1], e=(state.page<total)?pages[state.page]:pool.length;
    grid.innerHTML='';
    for(var k=s;k<e;k++) grid.insertAdjacentHTML('beforeend', makeCard(pool[k]));
    pgNav(pager, state.page, total, pool.length, function(np){ state.page=np; onpage(); });
  }

  // ================= DISCOVER =================
  var st={cat:'mov',rank:'popularity',mode:'trending',limit:25,page:1,updated:'just now'};
  function score(it){return it.pop*1.0 + Math.log((it.seeders||0)+1)*9 + Math.max(0,30-it.added)*0.6;}
  function sortList(list){
    var a=list.slice(), k=st.rank;
    if(k==='popularity') a.sort(function(x,y){return score(y)-score(x);});
    else if(k==='seeders') a.sort(function(x,y){return y.seeders-x.seeders;});
    else if(k==='recency') a.sort(function(x,y){return x.added-y.added;});
    else if(k==='rating') a.sort(function(x,y){return (y.rating||0)-(x.rating||0);});
    return a;
  }
  function discCard(it){
    var ar=it.ar||(it.logo?1.9:(it.cat==='mov'?0.675:it.cat==='tv'?0.7:0.72)), W=Math.round(180*ar);
    var posterCls='mk-dcposter'+(it.logo?' mk-logo':'');
    var c=cov(it.u);
    var img=c?imgTag(c,it.title):'<div class="mk-noart" style="width:100%;height:100%;display:flex;align-items:center;justify-content:center;text-align:center;padding:6px;background:linear-gradient(160deg,hsl('+(h(it.title||'')%360)+',34%,42%),hsl('+((h(it.title||'')+40)%360)+',34%,26%))"><span class="mk-noartt">'+esc(it.title)+'</span></div>';
    var badge, btn;
    if(it.state==='own'){ badge='<span class="mk-state mk-state-own">✓ In library</span>'; btn='<div class="mk-ownbtn">On disk</div>'; }
    else if(it.state==='prog'){ badge='<span class="mk-state mk-state-prog">↓ '+it.prog+'%</span>'; btn='<div class="mk-ownbtn" style="color:#cb4b16;border-color:rgba(203,75,22,.4)">Downloading…</div>'; }
    else { badge='<span class="mk-state mk-state-dl">↓ NEW</span>'; btn='<div class="mk-getbtn" data-dl="'+it.id+'">Download</div>'; }
    var sub=esc(it.sub)+((st.rank==='recency'||st.mode==='newest')?' · '+relAdded(it.added):'');
    return '<div class="mk-dccard'+(it.state==='own'?' mk-owned':'')+'" data-disc="'+it.id+'" style="width:'+W+'px"><div class="'+posterCls+'" style="width:'+W+'px;height:180px">'+img+badge+
      '<span class="mk-srctag">'+esc(it.src)+'</span><span class="mk-seed">'+fseed(it.seeders)+'</span>'+
      '<div class="mk-dchover">'+btn+'</div></div>'+
      '<div class="mk-dctitle">'+esc(it.title)+'</div><div class="mk-dcsub">'+sub+'</div></div>';
  }
  function discToolbar(){
    var chips=CATS.map(function(c){return '<label class="mk-chip'+(st.cat===c[0]?' act':'')+'" data-cat="'+c[0]+'">'+c[1]+'</label>';}).join('');
    var modeSeg='<span class="mk-seg"><label class="'+(st.mode==='trending'?'act':'')+'" data-mode="trending">Trending</label>'+
      (newestCat(st.cat)?'<label class="'+(st.mode==='newest'?'act':'')+'" data-mode="newest">Newest</label>':'')+'</span>';
    var rank='<span class="mk-rankby">Rank by: <select id="dl-rank">'+
      [['popularity','Popularity'],['seeders','Seeders'],['recency','Recency'],['rating','Rating']].map(function(o){return '<option value="'+o[0]+'"'+(st.rank===o[0]?' selected':'')+'>'+o[1]+'</option>';}).join('')+'</select></span>';
    var show='<span class="mk-ctllbl">Show</span><span class="mk-seg">'+
      [10,25,50,100].map(function(n){return '<label class="'+(st.limit===n?'act':'')+'" data-n="'+n+'">'+n+'</label>';}).join('')+'</span>';
    var refresh='<span class="mk-refgrp"><span class="mk-upd" id="dl-upd">updated '+st.updated+'</span><button class="mk-refresh" id="dl-refresh"><span class="mk-ricon"></span>Refresh</button></span>';
    var srcSel='<span class="mk-rankby">Source: <select id="dl-source">'+(DISC_SOURCES[st.cat]||[]).map(function(o){return '<option value="'+o[0]+'"'+(curSrc()===o[0]?' selected':'')+'>'+o[1]+'</option>';}).join('')+'</select></span>';
    return '<div class="mk-toolbar mk-dtoolbar"><div class="mk-chipset">'+chips+'</div><div class="mk-ctlset">'+srcSel+modeSeg+rank+show+refresh+'</div></div>';
  }
  // ---- live Discover feed (fetched from the local helper, which scrapes the real
  //      torrent sites server-side; "Show 10/25/50" slices the real top-50 pool) ----
  var discData={}, discIndex={}, discFresh=false, discMeta={};
  // per-category trend-source switcher (each fetched live by the helper from the real source)
  var DISC_SOURCES={ mov:[['tmdb','TMDB Trending'],['tmdb_popular','TMDB Popular'],['imdb','IMDb Popular'],['yts','YTS']],
    tv:[['tmdb','TMDB Trending'],['tmdb_airing','TMDB Airing'],['imdb','IMDb Popular'],['tpb','Pirate Bay']],
    ad:[['javdb','javdb rankings'],['mgstage','MGStage'],['sukebei','sukebei']], vrc:[['sukebei','sukebei'],['mgstage','MGStage VR']] };
  var discSrc={mov:'tmdb',tv:'tmdb',ad:'javdb',vrc:'sukebei'};
  function curSrc(){ return discSrc[st.cat]||(DISC_SOURCES[st.cat]&&DISC_SOURCES[st.cat][0][0])||''; }
  function dkey(){ return st.cat+'|'+st.mode+'|'+curSrc(); }
  function discFind(id){ return discIndex[id] || ITEMS.filter(function(x){return x.id===id;})[0]; }
  function srcLabel(c){ var L={tmdb:'TMDB Trending',yts:'YTS',tpb:'The Pirate Bay',javdb:'javdb',mgstage:'MGStage',sukebei:'sukebei'}; return L[curSrc()]||curSrc(); }
  function ownedKeys(){
    var s={};
    ITEMS.forEach(function(i){ if(i.state==='own'&&i.title) s[i.title.toUpperCase()]=1; });
    ['mov','tv','ad','vrc'].forEach(function(c){ var u=userLib[c]; if(u&&u.files) u.files.forEach(function(f){
      var t=(IDENT[f.fname]&&IDENT[f.fname].title)||f.title||''; if(t) s[t.toUpperCase()]=1;
      var m=(f.fname||'').match(/([A-Za-z]{2,6})[-_ ]?(\d{2,5})/); if(m) s[(m[1]+'-'+m[2]).toUpperCase()]=1;
    }); });
    return s;
  }
  function ingestDisc(items){
    var own=ownedKeys();
    items.forEach(function(it,i){
      it.logo=false;
      it.pop=(items.length-i)*2+(it.seeders||0)/50;
      if(it.added==null) it.added=i;
      it.rating=it.rating||0;
      if(it.cover){ COVERS[it.cover]=it.cover; it.u=it.cover; } else { it.u=''; }
      var k1=(it.code||'').toUpperCase(), k2=(it.title||'').toUpperCase();
      if(own[k1]||own[k2]) it.state='own';
      discIndex[it.id]=it;
    });
    return items;
  }
  function staticPool(){ return ingestDisc(sortList(ITEMS.filter(function(i){return i.cat===st.cat;})).slice(0,50)); }
  function discPaint(){
    var grid=document.getElementById('disc-grid'), pager=document.getElementById('disc-pager'); if(!grid)return;
    var pool=sortList((discData[dkey()]||[]).slice()).slice(0,st.limit);
    var upd=document.getElementById('dl-upd'); if(upd) upd.textContent='updated '+st.updated;
    paginate(grid, pager, pool, st, discCard, discPaint);
    if(!pool.length) grid.innerHTML='<div class="mk-empty">No live results right now — try Refresh.</div>';
    grid.querySelectorAll('[data-disc]').forEach(function(el){el.onclick=function(){openDiscPreview(el.getAttribute('data-disc'));};});
    grid.querySelectorAll('[data-dl]').forEach(function(el){el.onclick=function(e){e.stopPropagation();openDl(el.getAttribute('data-dl'));};});
    // lazily fill each visible card's seed badge with the REAL aggregated top-seeder count
    grid.querySelectorAll('[data-disc]').forEach(function(el){
      var it=discFind(el.getAttribute('data-disc')), badge=el.querySelector('.mk-seed'); if(!it||!badge)return;
      loadSeeds(it, function(d){ if(!d||!document.body.contains(badge))return;
        badge.textContent=fseed(d.topSeed||0); badge.classList.add('mk-seed-live');
        if(d.sources&&Object.keys(d.sources).length) badge.title=(d.topSeed||0)+' top seeders · '+Object.keys(d.sources).map(function(k){return k+': '+d.sources[k];}).join(', ');
      });
    });
  }
  function renderDisc(){
    var root=document.getElementById('disc-root'); if(!root)return;
    root.innerHTML=discToolbar()+'<div class="mk-page"><div class="mk-grid2" id="disc-grid"></div><div class="mk-detail2" id="disc-detail"></div></div><div class="mk-pager" id="disc-pager"></div>';
    wireDisc(root);
    var key=dkey();
    if(discData[key] && !discFresh){ discPaint(); return; }
    if(!window.__PROXY__){ discData[key]=staticPool(); st.updated='samples'; discPaint(); return; }
    var g0=document.getElementById('disc-grid');
    if(g0) g0.innerHTML='<div class="mk-empty"><span class="mk-spin" style="border-color:rgba(127,127,127,.35);border-top-color:var(--accent)"></span> &nbsp;Fetching live results from '+srcLabel(st.cat)+'…</div>';
    var fresh=discFresh; discFresh=false;
    var url=window.__PROXY__+'/discover?cat='+st.cat+'&mode='+st.mode+'&n=100&source='+encodeURIComponent(curSrc())+(fresh?'&fresh=1':'');
    var myKey=key, ctl=new AbortController(), to=setTimeout(function(){try{ctl.abort();}catch(e){}},60000);
    fetch(url,{signal:ctl.signal}).then(function(r){return r.json();}).then(function(d){
      clearTimeout(to);
      if(d&&d.ok&&d.items&&d.items.length){ discData[myKey]=ingestDisc(d.items); st.updated=d.updated||'just now'; }
      else { discData[myKey]=staticPool(); st.updated='samples (live empty)'; }
      if(dkey()===myKey && document.getElementById('disc-grid')) discPaint();
    }).catch(function(){
      clearTimeout(to);
      discData[myKey]=staticPool(); st.updated='samples (helper offline)';
      if(dkey()===myKey && document.getElementById('disc-grid')) discPaint();
    });
  }
  function wireDisc(root){
    root.querySelectorAll('[data-cat]').forEach(function(el){el.onclick=function(){st.cat=el.getAttribute('data-cat'); st.page=1; if(!newestCat(st.cat)&&st.mode==='newest'){st.mode='trending';st.rank='popularity';} renderDisc();};});
    root.querySelectorAll('[data-mode]').forEach(function(el){el.onclick=function(){st.mode=el.getAttribute('data-mode'); st.rank=(st.mode==='newest'?'recency':'popularity'); st.page=1; renderDisc();};});
    root.querySelectorAll('[data-n]').forEach(function(el){el.onclick=function(){st.limit=parseInt(el.getAttribute('data-n'),10); st.page=1; renderDisc();};});
    var sel=root.querySelector('#dl-rank'); if(sel) sel.onchange=function(){st.rank=sel.value; st.page=1; renderDisc();};
    var ssel=root.querySelector('#dl-source'); if(ssel) ssel.onchange=function(){ discSrc[st.cat]=ssel.value; st.page=1; renderDisc(); };
    root.querySelectorAll('[data-disc]').forEach(function(el){el.onclick=function(){openDiscPreview(el.getAttribute('data-disc'));};});
    root.querySelectorAll('[data-dl]').forEach(function(el){el.onclick=function(e){e.stopPropagation();openDl(el.getAttribute('data-dl'));};});
    var rb=root.querySelector('#dl-refresh'); if(rb) rb.onclick=function(){refresh(rb);};
  }
  function discPanelHTML(it, meta){
    var ar=it.ar||(it.logo?1.9:(it.cat==='mov'?0.675:it.cat==='tv'?0.7:0.72));
    var poster=panelPoster(cov(it.u),ar,it.logo,it.title);
    var st2= it.state==='own'?'<span class="mk-fact ok">in library</span>': it.state==='prog'?'<span class="mk-fact warn">downloading '+it.prog+'%</span>':'<span class="mk-fact">available to download</span>';
    var date=(meta&&meta.date)||'', rt=(meta&&meta.runtime)||'';
    var sc=seedCache[it.id];
    var seedTxt=sc?(fseed(sc.topSeed||0)+(Object.keys(sc.sources||{}).length?' ('+esc(Object.keys(sc.sources).join('/'))+')':'')):fseed(it.seeders);
    var facts='<div class="mk-facts"><span class="mk-fact">'+esc(it.sub)+'</span>'+
      (date?'<span class="mk-fact">📅 '+esc(date)+'</span>':'')+(rt?'<span class="mk-fact">⏱ '+esc(rt)+'</span>':'')+
      '<span class="mk-fact" id="disc-seedfact" title="top seeders across seeder sites">↓ '+seedTxt+'</span><span class="mk-fact">'+esc(it.src)+'</span>'+st2+'</div>';
    function chips(s){ return (s||'').split(/[,\/]/).map(function(x){x=x.trim();return x?'<span class="mk-chiplet">'+esc(x)+'</span>':'';}).join(''); }
    var ja=(meta&&meta.jatitle)?'<div class="mk-dja">'+esc(meta.jatitle)+'</div>':'';
    var castv=meta&&(meta.cast_ja||meta.cast);
    var sec=castv?'<div class="mk-dsec">出演 · Cast</div><div class="mk-chips'+(meta.cast_ja?' mk-ja':'')+'">'+chips(meta.cast_ja||meta.cast)+'</div>':'';
    var act= it.state==='new'?'<div class="mk-btnrow" style="margin-top:12px"><span class="mk-btn mk-btn-p" id="disc-dl">Download…</span></div>':'';
    return '<div class="mk-close" id="disc-close">✕</div>'+poster+'<div class="mk-dtitle">'+esc(it.title)+'</div>'+ja+facts+sec+act;
  }
  function openDiscPreview(id){
    var it=discFind(id); if(!it)return;
    var d=document.getElementById('disc-detail'); if(!d)return;
    function wire(){
      var c=document.getElementById('disc-close'); if(c) c.onclick=function(){d.classList.remove('show');};
      var b=document.getElementById('disc-dl'); if(b) b.onclick=function(){ d.classList.remove('show'); openDl(it.id); };
    }
    var jav=(it.cat==='ad'||it.cat==='vrc'), cached=discMeta[it.id];
    d.innerHTML=discPanelHTML(it, cached); d.classList.add('show'); wire();
    loadSeeds(it, function(sd){ if(!sd)return; var sf=document.getElementById('disc-seedfact');
      if(sf&&d.classList.contains('show')){ sf.innerHTML='↓ '+fseed(sd.topSeed||0)+(Object.keys(sd.sources||{}).length?' ('+esc(Object.keys(sd.sources).join('/'))+')':'');
        sf.title=(sd.topSeed||0)+' top seeders · '+Object.keys(sd.sources||{}).map(function(k){return k+': '+sd.sources[k];}).join(', '); } });
    if(jav && !cached && window.__PROXY__ && it.code){
      var note=document.createElement('div'); note.className='mk-dsec'; note.textContent='fetching Japanese title + cast…'; d.appendChild(note);
      var cid=cidOf(it.u);
      fetch(window.__PROXY__+'/meta?cid='+encodeURIComponent(cid)+'&code='+encodeURIComponent(it.code)+'&cat='+it.cat).then(function(r){return r.json();}).then(function(m){
        m=m||{}; discMeta[it.id]=m;
        if(d.classList.contains('show')){ d.innerHTML=discPanelHTML(it,m); wire(); }
      }).catch(function(){ if(note) note.textContent='metadata lookup failed'; });
    }
  }
  function refresh(btn){
    if(btn){ btn.disabled=true; btn.innerHTML='<span class="mk-spin"></span> refreshing…'; }
    delete discData[dkey()]; discFresh=true; st.page=1;
    renderDisc();
  }

  // ================= LIBRARY =================
  var lib={cat:'mov',page:1,q:'',rank:'alpha',dir:'asc'}; var groupParts=true;
  function noart(title){ var x=h(title); return '<div class="mk-poster mk-noart" style="background:linear-gradient(160deg,hsl('+(x%360)+',34%,42%),hsl('+((x+40)%360)+',34%,26%))"><span class="mk-noartt">'+esc(title)+'</span></div>'; }
  function panelPoster(cover,ar,logo,title){
    var H=200, W=Math.round(H*(ar||0.7));
    if(cover) return '<div class="mk-dposter'+(logo?' mk-logo':'')+'" style="width:'+W+'px;height:'+H+'px"><img src="'+cover+'" alt=""></div>';
    return '<div class="mk-dposter mk-noart" style="width:'+Math.round(H*0.7)+'px;height:'+H+'px;background:linear-gradient(160deg,hsl('+(h(title)%360)+',34%,42%),hsl('+((h(title)+40)%360)+',34%,26%))"></div>';
  }
  function defAr(cat){ return cat==='mov'?0.675:(cat==='tv'?0.7:0.72); }
  function disp(it){ var info=it.user?IDENT[it.fname]:null;
    if(info&&info.part) return { title:info.grp, sub:'VR · part '+info.part, cover:(GRPCOVER[info.grp]||''), logo:false, tag:'VR', ident:true, part:info.part, ar:(GRPAR[info.grp]||0.72) };
    return { title:(info?info.title:it.title), sub:(info?info.sub:it.sub),
      cover:(info&&info.cover)?info.cover:(it.user?'':cov(it.u)),
      logo:(info?!!info.logo:!!it.logo), tag:(it.cat==='tv'?'TV':it.cat==='vrc'?'VR':''), ident:!!info,
      part:((info&&info.pa&&!groupParts)?info.pa:''),
      ar:((info&&info.ar)?info.ar:defAr(it.cat)) }; }
  var COVER_H=180;
  function libCard(it){ var d=disp(it);
    var ar=d.ar||0.72, W=Math.round(COVER_H*ar), Wn=Math.round(COVER_H*0.7);
    var partb = d.part ? '<span class="mk-partbadge">'+esc(d.part)+'</span>' : '';
    var ov='<div class="mk-hover-actions"><span class="mk-hbtn" data-play="'+it.id+'">▶ Play</span></div>';
    var poster = d.cover
      ? '<div class="mk-poster'+(d.logo?' mk-logo':'')+'" style="width:'+W+'px;height:'+COVER_H+'px"><img loading="lazy" src="'+d.cover+'" alt="">'+(d.tag?'<span class="mk-tag">'+d.tag+'</span>':'')+partb+ov+'</div>'
      : '<div class="mk-poster mk-noart" style="width:'+Wn+'px;height:'+COVER_H+'px;background:linear-gradient(160deg,hsl('+(h(d.title)%360)+',34%,42%),hsl('+((h(d.title)+40)%360)+',34%,26%))"><span class="mk-noartt">'+esc(d.title)+'</span>'+ov+'</div>';
    return '<div class="mk-card" data-lib="'+it.id+'" style="width:'+(d.cover?W:Wn)+'px">'+poster+'<div class="mk-cap">'+esc(d.title)+'</div><div class="mk-sub">'+esc(d.sub)+'</div></div>';
  }
  function libToolbar(){
    var chips=CATS.map(function(c){return '<label class="mk-chip'+(lib.cat===c[0]?' act':'')+'" data-libcat="'+c[0]+'">'+c[1]+'</label>';}).join('');
    var search='<input id="lib-search" class="mk-searchbar" placeholder="Search title or code…" value="'+esc(lib.q||'')+'">';
    var rank='<span class="mk-ctllbl">Rank</span><span class="mk-seg"><label class="'+(lib.rank==='alpha'?'act':'')+'" data-rank="alpha">Title</label><label class="'+(lib.rank==='release'?'act':'')+'" data-rank="release">Release</label></span><span class="mk-seg"><label class="'+(lib.dir==='asc'?'act':'')+'" data-dir="asc">↑ Asc</label><label class="'+(lib.dir==='desc'?'act':'')+'" data-dir="desc">↓ Desc</label></span>';
    var right='';
    if(lib.cat==='vrc') right='<span class="mk-ctllbl">Multi-part</span><span class="mk-seg"><label class="'+(groupParts?'act':'')+'" data-grp="1">Grouped</label><label class="'+(!groupParts?'act':'')+'" data-grp="0">All parts</label></span>';
    return '<div class="mk-toolbar mk-dtoolbar"><div class="mk-chipset">'+chips+'</div><div class="mk-ctlset">'+search+rank+right+'</div></div>';
  }
  function libPool(){ var u=userLib[lib.cat]; if(u&&u.files) return u.files.filter(function(it){ var info=IDENT[it.fname]; if(!info||!info.hide) return true; if(lib.cat==='vrc' && !groupParts && info.part) return true; return false; }); return ITEMS.filter(function(i){return i.cat===lib.cat && i.state==='own';}); }
  function libDate(it){ var info=it.user?IDENT[it.fname]:null; var d=(info&&info.date)||''; if(!d){ var m=(it.sub||'').match(/(19|20)\d{2}/); d=m?m[0]:''; } return d; }
  function libFilteredPool(){
    var pool=libPool(); var q=(lib.q||'').trim().toLowerCase();
    if(q) pool=pool.filter(function(it){ var d=disp(it); return (d.title||'').toLowerCase().indexOf(q)>=0 || (it.fname||'').toLowerCase().indexOf(q)>=0; });
    pool=pool.slice();
    function partOf(it){ var info=it.user?IDENT[it.fname]:null; return (info&&info.part)||''; }
    var dir=(lib.dir==='desc')?-1:1;
    pool.sort(function(a,b){
      var k=(lib.rank==='release')?(libDate(a)||'').localeCompare(libDate(b)||''):0;
      if(k===0) k=(disp(a).title||'').localeCompare(disp(b).title||'');   // group parts by work
      if(k!==0) return dir*k;                                              // direction: works only
      return (partOf(a)||'').localeCompare(partOf(b)||'');                 // within a work: always A,B,C
    });
    return pool;
  }
  function libFill(){
    var grid=document.getElementById('lib-grid'), pager=document.getElementById('lib-pager'); if(!grid)return;
    var pool=libFilteredPool();
    paginate(grid,pager,pool,lib,libCard,libFill);
    if(pool.length===0){ var u=userLib[lib.cat]; grid.innerHTML='<div class="mk-empty">'+(lib.q?('No matches for “'+esc(lib.q)+'”.'):(u?('No videos found in '+esc(u.name)+'.'):'No items in this category yet. Set a folder in Settings → Library.'))+'</div>'; }
    grid.querySelectorAll('[data-lib]').forEach(function(el){el.onclick=function(){openLib(el.getAttribute('data-lib'));};
      el.oncontextmenu=function(e){ e.preventDefault(); e.stopPropagation(); var id=el.getAttribute('data-lib');
        showCtx(e.clientX,e.clientY,[
          {label:'Reveal in Explorer', onClick:function(){revealFile(id);}},
          {sep:true},
          {label:'Delete to Recycle Bin', danger:true, onClick:function(){deleteItem(id);}}
        ]); };
    });
    grid.querySelectorAll('[data-play]').forEach(function(el){el.onclick=function(e){e.stopPropagation(); playFile(el.getAttribute('data-play'));};});
    lazyLibCovers();
  }
  var _libCovT;
  function lazyLibCovers(){   // fetch real covers for the visible Library cards (cascade for ad/vr, TMDB for mov/tv)
    var grid=document.getElementById('lib-grid'); if(!grid||!window.__PROXY__) return;
    var u=userLib[lib.cat]; if(!u||!u.files) return;
    var byId={}; u.files.forEach(function(it){ byId[it.id]=it; });
    grid.querySelectorAll('[data-lib]').forEach(function(el){
      var it=byId[el.getAttribute('data-lib')]; if(!it||it.u||it._ct) return;
      it._ct=1;
      var url=(it.cat==='ad'||it.cat==='vrc')
        ? (it.code? window.__PROXY__+'/cover?code='+encodeURIComponent(it.code) : null)
        : window.__PROXY__+(it.cat==='tv'?'/tv':'/movie')+'?title='+encodeURIComponent(it.title||'')+'&year='+encodeURIComponent(it.year||'');
      if(!url) return;
      fetch(url).then(function(r){return r.json();}).then(function(d){
        var cov=d&&(d.cover||(d.meta&&d.meta.cover))||'';
        if(cov){ it.u=cov; it.cover=cov; COVERS[cov]=cov;
          clearTimeout(_libCovT); _libCovT=setTimeout(function(){ if(document.getElementById('lib-grid')) libFill(); }, 500); }
      }).catch(function(){});
    });
  }
  function renderLib(){
    var root=document.getElementById('lib-root'); if(!root)return;
    root.innerHTML=libToolbar()+'<div class="mk-page"><div class="mk-grid2" id="lib-grid"></div><div class="mk-detail2" id="lib-detail"></div></div><div class="mk-pager" id="lib-pager"></div>';
    root.querySelectorAll('[data-libcat]').forEach(function(el){el.onclick=function(){lib.cat=el.getAttribute('data-libcat'); lib.q=''; lib.page=1; renderLib();};});
    root.querySelectorAll('[data-grp]').forEach(function(el){el.onclick=function(){groupParts=el.getAttribute('data-grp')==='1'; lib.page=1; renderLib();};});
    root.querySelectorAll('[data-rank]').forEach(function(el){el.onclick=function(){lib.rank=el.getAttribute('data-rank'); lib.page=1; renderLib();};});
    root.querySelectorAll('[data-dir]').forEach(function(el){el.onclick=function(){lib.dir=el.getAttribute('data-dir'); lib.page=1; renderLib();};});
    var sb=document.getElementById('lib-search'); if(sb) sb.oninput=function(){ lib.q=sb.value; lib.page=1; libFill(); };
    libFill();
  }
  function openLib(id){
    var it=findItem(id); if(!it)return;
    var d=document.getElementById('lib-detail'); if(!d)return;
    var dd=disp(it);
    var poster = panelPoster(dd.cover, dd.ar, dd.logo, dd.title);
    var info=it.user?IDENT[it.fname]:null;
    var jav=(it.cat==='ad'||it.cat==='vrc');
    var loc=it.user?((userLib[it.cat]?userLib[it.cat].name:it.cat)+'\\'+esc(it.fname)):(DEST[it.cat]+'\\'+esc(dd.title)+'\\');
    var ym=(dd.sub||'').match(/(19|20)\d{2}/);
    var date=(info&&info.date)||(ym?ym[0]:''), rt=(info&&info.runtime)||'';
    function chips(s){ return (s||'').split(/[,\/]/).map(function(x){x=x.trim();return x?'<span class="mk-chiplet">'+esc(x)+'</span>':'';}).join(''); }
    var ja=(info&&info.jatitle)?'<div class="mk-dja">'+esc(info.jatitle)+'</div>':'';
    var facts='<div class="mk-facts">'+(date?'<span class="mk-fact">📅 '+esc(date)+'</span>':'')+(rt?'<span class="mk-fact">⏱ '+esc(rt)+'</span>':'')+
      (info?'<span class="mk-fact ok">identified</span>':'<span class="mk-fact warn">needs review</span>')+'</div>';
    var sec='';
    if((it.cat==='mov'||it.cat==='tv')&&info&&info.genre) sec+='<div class="mk-dsec">Genre</div><div class="mk-chips">'+chips(info.genre)+'</div>';
    if(jav&&info&&(info.cast_ja||info.cast)) sec+='<div class="mk-dsec">出演 · Cast</div><div class="mk-chips'+(info.cast_ja?' mk-ja':'')+'">'+chips(info.cast_ja||info.cast)+'</div>';
    d.innerHTML='<div class="mk-close" id="lib-close">✕</div>'+poster+'<div class="mk-dtitle">'+esc(dd.title)+'</div>'+ja+facts+sec+
      '<div class="mk-dsec">Location</div><div class="mk-dpath">'+loc+'</div>'+
      '<div class="mk-btnrow" style="margin-top:12px"><span class="mk-btn mk-btn-p">Play</span><span class="mk-btn">Open folder</span><span class="mk-btn">Re-identify</span></div>';
    d.classList.add('show');
    var c=document.getElementById('lib-close'); if(c) c.onclick=function(){d.classList.remove('show');};
    var needJa=jav&&info&&!info.jatitle&&(window.__PROXY__||(KEYS.dmmApi&&KEYS.dmmAff));
    var needTmdb=it.cat==='mov'&&info&&!info.genre&&KEYS.tmdb;
    if(needJa||needTmdb){
      var note=document.createElement('div'); note.className='mk-dsec'; note.textContent=needJa?'fetching Japanese title + cast…':'fetching from TMDB…'; d.appendChild(note);
      var job = needJa ? (window.__PROXY__?proxyFill(it,info):dmmFill(it,info)) : tmdbFill(it,info);
      job.then(function(ok){ if(ok&&d.classList.contains('show')) openLib(id); else note.textContent='lookup returned nothing'; });
    }
  }

  // ---- dynamic download window ----
  function releasesFor(it){
    var base=it.runtime||120, t=it.title.replace(/[: ]+/g,'.');
    if(it.cat==='vrc') return [{n:it.title+' [8K VR]',q:'8K',gb:(base*0.16).toFixed(1),s:Math.round(it.seeders*0.5)},{n:it.title+' [4K VR]',q:'4K',gb:(base*0.09).toFixed(1),s:it.seeders}];
    if(it.cat==='ad') return [{n:it.title+' [1080p]',q:'1080p',gb:(base*0.045).toFixed(1),s:it.seeders},{n:it.title+' [720p]',q:'720p',gb:(base*0.022).toFixed(1),s:Math.round(it.seeders*1.6)}];
    return [{n:t+'.2160p.WEB-DL.x265',q:'2160p',gb:(base*0.16).toFixed(1),s:Math.round(it.seeders*0.4)},{n:t+'.1080p.BluRay.x264',q:'1080p',gb:(base*0.085).toFixed(1),s:it.seeders},{n:t+'.720p.WEBRip',q:'720p',gb:(base*0.042).toFixed(1),s:Math.round(it.seeders*0.7)}];
  }
  // ---- live seeder lookup: real releases + magnets queried per item from the seeder sites
  //      (TPB/YTS for movies+TV, sukebei for adult/VR), merged + sorted by seeders ----
  var seedCache={};
  function dlYear(it){ var m=(''+(it.year||it.sub||'')).match(/(19|20)\d{2}/); return m?m[0]:''; }
  function loadSeeds(it, cb){
    if(seedCache[it.id]){ cb(seedCache[it.id]); return; }
    if(!window.__PROXY__){ cb(null); return; }
    var url=window.__PROXY__+'/seeders?cat='+it.cat+'&title='+encodeURIComponent(it.title||'')+'&code='+encodeURIComponent(it.code||'')+'&year='+encodeURIComponent(dlYear(it));
    fetch(url).then(function(r){return r.json();}).then(function(d){
      seedCache[it.id]=(d&&d.ok)?d:{releases:[],topSeed:0,sources:{},count:0}; cb(seedCache[it.id]);
    }).catch(function(){ cb(null); });
  }
  // ---- real downloads via the Tauri/librqbit bridge (no-op preview in a plain browser) ----
  function mkToast(msg){
    var t=document.getElementById('mk-toast');
    if(!t){ t=document.createElement('div'); t.id='mk-toast'; t.className='mk-toast'; (document.querySelector('.mk-app')||document.body).appendChild(t); }
    t.textContent=msg; t.classList.add('show'); clearTimeout(t._to); t._to=setTimeout(function(){ t.classList.remove('show'); },2200);
  }
  function reflectDl(it){
    var el=document.querySelector('#disc-grid [data-disc="'+it.id+'"]'); if(!el)return;
    var badge=el.querySelector('.mk-state'); if(!badge)return;
    if(it.state==='own'){ badge.className='mk-state mk-state-own'; badge.textContent='✓ In library'; }
    else { badge.className='mk-state mk-state-prog'; badge.textContent='↓ '+(it.prog||0)+'%'; }
  }
  var activeDl={};
  function renderDownloads(){
    var ids=Object.keys(activeDl);
    var act=ids.filter(function(k){return activeDl[k].state!=='done';});
    var spd=0; act.forEach(function(k){ spd+=(activeDl[k].speed||0); });
    var sum=document.querySelector('.mk-dsum'); if(sum) sum.textContent=act.length?(act.length+' downloading'):(ids.length?'downloads complete':'no active downloads');
    var sp=document.querySelector('.mk-dspeed'); if(sp) sp.textContent='↓ '+spd.toFixed(1)+' MB/s';
    var box=document.querySelector('.mk-dcards');
    if(box){ box.innerHTML = ids.length ? ids.map(function(k){ var d=activeDl[k], done=d.state==='done', pct=done?100:(d.prog||0);
      return '<div class="mk-dcard"><div class="mk-dcname">'+esc(d.title||k)+(done?' ✓':'')+'</div><div class="mk-dctrack"><span class="mk-dcfill" style="width:'+pct+'%;background:'+(done?'#859900':'var(--accent)')+'"></span></div></div>';
    }).join('') : '<div class="mk-dcard" style="opacity:.55"><div class="mk-dcname">No active downloads</div></div>'; }
  }
  function fmtBytes(b){ b=b||0; var u=['B','KB','MB','GB','TB']; var i=0; while(b>=1024&&i<u.length-1){b/=1024;i++;} return (i>=3?b.toFixed(1):Math.round(b))+' '+u[i]; }
  function renderDash(){   // real disk usage + library counts + active downloads on the Dashboard
    var stats=document.querySelectorAll('.mk-view-dash .mk-stat');
    var act=Object.keys(activeDl).filter(function(k){return activeDl[k].state!=='done';}).length;
    if(stats[0]){ var n0=stats[0].querySelector('.mk-statn'), l0=stats[0].querySelector('.mk-statl'); if(n0)n0.textContent=act; if(l0)l0.textContent='active downloads'; }
    if(!window.__PROXY__) return;
    fetch(window.__PROXY__+'/stats').then(function(r){return r.json();}).then(function(d){
      if(!d||!d.ok)return; var disks=d.disks||{}, online=0, files=0;
      var cats=[['mov','Movies'],['tv','TV'],['ad','Adult'],['vrc','VR']];
      var rows=document.querySelectorAll('.mk-view-dash .mk-disk');
      cats.forEach(function(cv,i){ var info=disks[cv[0]]||{}, row=rows[i]; if(!row)return;
        if(info.online) online++; files+=(info.files||0);
        var lab=row.querySelector('.mk-dlabel'), fill=row.querySelector('.mk-dfill'), free=row.querySelector('.mk-dfree');
        if(lab) lab.textContent=(info.path||cv[1])+(info.online?'':' · offline');
        if(info.total){ var used=Math.round((1-info.free/info.total)*100); if(fill) fill.style.width=used+'%';
          if(free) free.textContent=fmtBytes(info.free)+' free of '+fmtBytes(info.total)+' · '+(info.files||0)+' files'; }
        else { if(fill) fill.style.width='0%'; if(free) free.textContent=info.online?((info.files||0)+' files'):'offline'; }
      });
      if(stats[1]){ var n1=stats[1].querySelector('.mk-statn'), l1=stats[1].querySelector('.mk-statl'); if(n1)n1.textContent=files; if(l1)l1.textContent='files in library'; }
      if(stats[2]){ var n2=stats[2].querySelector('.mk-statn'), l2=stats[2].querySelector('.mk-statl'); if(n2)n2.textContent=online+' / 4'; if(l2)l2.textContent='folders online'; }
    }).catch(function(){});
  }
  function loadPaths(){   // point downloads at the user's real configured library folders
    if(!window.__PROXY__)return;
    fetch(window.__PROXY__+'/paths').then(function(r){return r.json();}).then(function(p){
      if(p&&typeof p==='object'){ ['mov','tv','ad','vrc'].forEach(function(c){ if(p[c]) DEST[c]=p[c]; }); }
    }).catch(function(){});
  }
  function loadLibrary(){   // replace the baked sample library with a live scan of the real folders
    if(!window.__PROXY__)return;
    Promise.all([
      fetch(window.__PROXY__+'/paths').then(function(r){return r.json();}).catch(function(){return {};}),
      fetch(window.__PROXY__+'/library').then(function(r){return r.json();}).catch(function(){return null;})
    ]).then(function(arr){
      var paths=arr[0]||{}, d=arr[1];
      if(!d||!d.ok||!d.items||!d.items.length)return;
      var byCat={mov:[],tv:[],ad:[],vrc:[]};
      d.items.forEach(function(it,i){ it.id=it.id||('lib'+i); it.u=it.cover||''; it.state='own'; if(byCat[it.cat]) byCat[it.cat].push(it); });
      ['mov','tv','ad','vrc'].forEach(function(c){ if(byCat[c].length){ userLib[c]={name:(paths[c]||c),path:(paths[c]||''),count:byCat[c].length,files:byCat[c]}; } });
      try{ saveUL(); }catch(e){}
      if(document.getElementById('lib-grid')) renderLib();
    }).catch(function(){});
  }
  function startDownload(it){
    var relBox=document.getElementById('dl-rel');
    var r=relBox?(relBox.querySelector('input:checked')||relBox.querySelector('input')):null;
    var magnet=r?(r.getAttribute('data-mag')||''):'';
    var T=window.__TAURI__;
    if(!magnet){ mkToast('No magnet for this release'); return; }
    if(T&&T.core){
      T.core.invoke('start_download',{magnet:magnet,dest:DEST[it.cat]||'',id:String(it.id),title:String(it.title||'')})
        .then(function(){ mkToast('Download started'); })
        .catch(function(e){ mkToast('Download failed'); console.error('start_download',e); });
    } else { mkToast('Download (preview only — run the app)'); }
    it.state='prog'; it.prog=0; reflectDl(it);
    activeDl[it.id]={title:it.title,prog:0,speed:0,state:'prog'}; renderDownloads();
  }
  function setupDlBridge(){
    renderDownloads();   // clear the sample drawer to the real (empty) state on load
    var T=window.__TAURI__; if(!T||!T.event)return;
    T.event.listen('download-progress',function(ev){
      var p=ev.payload||{}, pct=Math.round((p.progress||0)*100);
      var it=discFind(p.id); if(it){ it.prog=pct; it.state=(p.state==='done')?'own':'prog'; reflectDl(it); }
      activeDl[p.id]={title:(it&&it.title)||p.title||p.id, prog:pct, speed:p.speed_mbps||0, state:p.state}; renderDownloads();
    });
  }
  function openDl(id){
    var it=discFind(id); if(!it)return;
    document.getElementById('dl-cover').src=cov(it.u);
    document.getElementById('dl-title').textContent=it.title;
    document.getElementById('dl-sub').textContent=it.sub+' · from '+it.src;
    var relBox=document.getElementById('dl-rel'), filesBox=document.getElementById('dl-files'), confBox=document.getElementById('dl-confirm');
    relBox.innerHTML='<div class="mk-empty" style="padding:16px 8px"><span class="mk-spin" style="border-color:rgba(127,127,127,.35);border-top-color:var(--accent)"></span> &nbsp;Searching seeders…</div>';
    filesBox.innerHTML=''; confBox.innerHTML='';
    var cb=document.getElementById('mk-dlmodal'); if(cb) cb.checked=true;
    var sb=document.querySelector('.mk-mfoot .mk-btn-p'); if(sb) sb.onclick=function(){ startDownload(it); };
    loadSeeds(it, function(d){
      var rels=(d&&d.releases&&d.releases.length)?d.releases:null;
      if(!rels){ renderDlMock(it,relBox,filesBox,confBox); return; }
      var srcLabel=Object.keys(d.sources||{}).map(function(k){return k+' '+d.sources[k];}).join(' · ');
      document.getElementById('dl-sub').textContent=it.sub+' · '+d.count+' live releases ('+srcLabel+')';
      relBox.innerHTML=rels.map(function(r,i){return '<label class="dl-opt'+(i===0?' sel':'')+'"><input type="radio" name="dlrel" '+(i===0?'checked':'')+' data-mag="'+esc(r.magnet)+'" data-sz="'+esc(r.size||'')+'" data-nm="'+esc(r.name)+'"> '+esc(r.name)+' <span class="m">'+(r.quality?esc(r.quality)+' · ':'')+esc(r.size||'?')+' · '+esc(r.source)+' · '+fseed(r.seeders)+' seeders</span></label>';}).join('');
      function upd(){ var r=relBox.querySelector('input:checked')||relBox.querySelector('input'); if(!r)return;
        var sz=r.getAttribute('data-sz')||'', nm=r.getAttribute('data-nm')||it.title;
        filesBox.innerHTML='<label class="dl-opt sel"><input type="checkbox" checked> '+esc(nm)+' <span class="m">'+esc(sz)+'</span></label>';
        confBox.innerHTML='Title&nbsp; <b>'+esc(it.title)+'</b><br>Size&nbsp;&nbsp; <b>'+esc(sz||'?')+'</b><br>Magnet&nbsp; <b style="color:var(--accent)">ready ✓</b><br>Goes to&nbsp; <b>'+DEST[it.cat]+'\\</b>'; }
      relBox.querySelectorAll('.dl-opt').forEach(function(o){o.onclick=function(){relBox.querySelectorAll('.dl-opt').forEach(function(x){x.classList.remove('sel');});o.classList.add('sel');setTimeout(upd,0);};});
      upd();
    });
  }
  function renderDlMock(it,relBox,filesBox,confBox){
    var rels=releasesFor(it);
    document.getElementById('dl-sub').textContent=it.sub+' · from '+it.src+' (helper offline — sample data)';
    relBox.innerHTML=rels.map(function(r,i){return '<label class="dl-opt'+(i===0?' sel':'')+'"><input type="radio" name="dlrel" '+(i===0?'checked':'')+' data-gb="'+r.gb+'"> '+esc(r.n)+' <span class="m">'+r.q+' · '+r.gb+' GB · '+fseed(r.s)+' seeders</span></label>';}).join('');
    var single=(it.cat==='ad'||it.cat==='vrc'), t=it.title.replace(/[: ]+/g,'.');
    var files = single ? [[it.title+'.mp4', rels[0].gb+' GB', true]] : [[t+'.mkv',rels[0].gb+' GB',true]];
    filesBox.innerHTML=files.map(function(f){return '<label class="dl-opt'+(f[2]?' sel':'')+'"><input type="checkbox" '+(f[2]?'checked':'')+'> '+esc(f[0])+' <span class="m">'+f[1]+'</span></label>';}).join('');
    function upd(){ var r=relBox.querySelector('input:checked'); var gb=r?r.getAttribute('data-gb'):rels[0].gb;
      confBox.innerHTML='Title&nbsp; <b>'+esc(it.title)+'</b><br>Size&nbsp;&nbsp; <b>'+gb+' GB</b><br>Goes to&nbsp; <b>'+DEST[it.cat]+'\\</b>'; }
    relBox.querySelectorAll('.dl-opt').forEach(function(o){o.onclick=function(){relBox.querySelectorAll('.dl-opt').forEach(function(x){x.classList.remove('sel');});o.classList.add('sel');setTimeout(upd,0);};});
    upd();
  }

  function visible(el){ return el && el.offsetParent!==null; }
  var rTO;
  function onResize(){ clearTimeout(rTO); rTO=setTimeout(function(){ if(visible(document.getElementById('lib-root'))) renderLib(); if(visible(document.getElementById('disc-root'))) renderDisc(); },150); }
  document.addEventListener('DOMContentLoaded',function(){
    renderLib();
    setupDlBridge();
    loadPaths();
    loadLibrary();
    renderDash();
    var ndash=document.querySelector('.mk-nav-dash'); if(ndash) ndash.addEventListener('click',function(){setTimeout(renderDash,0);});
    var nd=document.querySelector('.mk-nav-disc'); if(nd) nd.addEventListener('click',function(){setTimeout(renderDisc,0);});
    var nl=document.querySelector('.mk-nav-lib'); if(nl) nl.addEventListener('click',function(){setTimeout(renderLib,0);});
    document.addEventListener('keydown',function(e){ if(e.key==='Escape'){ var cm=document.getElementById('mk-ctxmenu'); if(cm){ cm.remove(); return; } var dm=document.getElementById('mk-dlmodal'); if(dm&&dm.checked){ dm.checked=false; return; } var p=document.getElementById('mk-player'); if(p&&p.classList.contains('show')){ p.classList.remove('show'); var v=p.querySelector('video'); if(v){try{v.pause();}catch(x){}} return; } document.querySelectorAll('.mk-detail2.show').forEach(function(x){x.classList.remove('show');}); } });
    var dlmodal=document.querySelector('.mk-modal'); if(dlmodal) dlmodal.addEventListener('click',function(e){ if(!(e.target.closest&&e.target.closest('.mk-mbox'))){ var dm=document.getElementById('mk-dlmodal'); if(dm) dm.checked=false; } });
    document.addEventListener('click',function(e){ if(e.target.closest&&(e.target.closest('.mk-detail2')||e.target.closest('[data-lib],[data-disc]'))) return; document.querySelectorAll('.mk-detail2.show').forEach(function(x){x.classList.remove('show');}); });
    function prefillKeys(){ [['key-dmm-api','dmmApi'],['key-dmm-aff','dmmAff'],['key-tmdb','tmdb']].forEach(function(p){ var el=document.getElementById(p[0]); if(el&&KEYS[p[1]]) el.value=KEYS[p[1]]; }); }
    prefillKeys(); loadHelperKeys(prefillKeys);
    var ksv=document.getElementById('key-save'); if(ksv) ksv.onclick=function(){
      [['key-dmm-api','dmmApi'],['key-dmm-aff','dmmAff'],['key-tmdb','tmdb']].forEach(function(p){ var el=document.getElementById(p[0]); if(el){ KEYS[p[1]]=el.value.trim(); saveHelperKey(p[1],KEYS[p[1]]); } });
      saveKeys(); var st=document.getElementById('key-status'); if(st){ st.textContent='Saved ✓'; setTimeout(function(){st.textContent='';},2000); } };
    document.querySelectorAll('[data-pick]').forEach(function(b){ b.onclick=function(){ pickFolder(b.getAttribute('data-pick')); }; });
    document.querySelectorAll('[data-scan]').forEach(function(b){ b.onclick=function(){ scanFolder(b.getAttribute('data-scan')); }; });
    function fillPaths(){ ['mov','tv','ad','vrc'].forEach(function(cat){ var el=document.getElementById('path-'+cat); if(!el)return;
      var u=userLib[cat]; var v=av_paths[cat]||(u&&(u.path||u.name))||''; if(v) el.value=v;
      el.oninput=function(){ av_paths[cat]=el.value.trim(); savePaths(); saveHelperPath(cat,av_paths[cat]); };
      if(!isAbs(el.value) && window.__PROXY__ && u && u.name && u.files && u.files.length){
        var probe=''; for(var i=0;i<u.files.length;i++){ var fn=u.files[i].fname; if(fn && fn.indexOf('._')!==0){ probe=fn; break; } }
        var nm=(u.name||'').split(/[\\\/]/).pop();
        el.placeholder='locating '+nm+'… ';
        fetch(window.__PROXY__+'/findpath?cat='+cat+'&name='+encodeURIComponent(nm)+'&probe='+encodeURIComponent(probe)).then(function(r){return r.json();}).then(function(d){
          var e2=document.getElementById('path-'+cat); if(!e2)return;
          if(d&&d.ok&&d.path){ av_paths[cat]=d.path; savePaths(); if(!isAbs(e2.value)) e2.value=d.path; }
          else { e2.placeholder='full path e.g. D:\\'+nm+' (couldn’t auto-locate “'+nm+'”)'; }
        }).catch(function(){});
      }
    }); }
    loadHelperPaths(fillPaths);
    window.addEventListener('resize',onResize);
    var ts=document.getElementById('mk-theme-sel'), app=document.querySelector('.mk-app');
    if(ts&&app) ts.onchange=function(){ if(ts.value==='Solarized Light') app.removeAttribute('data-theme'); else app.setAttribute('data-theme',ts.value); };
  });
})();
