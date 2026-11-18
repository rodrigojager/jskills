'use strict';
// The only Orca-specific renderer code. The generated shared/ files are canonical web assets.
(() => {
  let api, context=null, revision=0;
  const main=document.querySelector('main'), run=document.querySelector('#run');
  let updateOffer=null,updateBusy=false;
  const updateStatus=document.querySelector('#updates-status'),checkButton=document.querySelector('#check-updates'),applyButton=document.querySelector('#apply-update'),approval=document.querySelector('#update-approval');
  async function checkUpdates(automatic=false) {
    if(updateBusy)return;updateBusy=true;checkButton.disabled=true;applyButton.disabled=true;
    if(!automatic)updateStatus.textContent='Checking stable releases…';
    try{
      updateOffer=await window.orca.plugin.request('updates.check',{automatic},{timeoutMs:65000});
      approval.hidden=true;
      if(updateOffer.state==='checked'){
        const native=updateOffer.cli;
        updateStatus.textContent=updateOffer.message || (updateOffer.available?`Version ${updateOffer.latest} is available (installed ${updateOffer.current}).`:`Plugin ${updateOffer.current} is up to date.`);
        if(native?.latest_version)updateStatus.textContent+=` CLI ${native.current_version}; latest ${native.latest_version}.`;
        else if(native?.error)updateStatus.textContent+=` CLI check: ${native.error}`;
        document.querySelector('#updates-label').textContent=updateOffer.available?'Update available':'Updates';
      }else updateStatus.textContent=updateOffer.message || 'Update status unavailable.';
      applyButton.hidden=!updateOffer.available;
    }catch(error){updateOffer=null;applyButton.hidden=true;updateStatus.textContent=`Could not check updates: ${error.message}`;}
    finally{updateBusy=false;checkButton.disabled=false;applyButton.disabled=false;}
  }
  async function applyUpdate() {
    if(updateBusy)return;
    // A cached notice must be revalidated in this worker before any installation.
    if(!updateOffer?.token)await checkUpdates();
    if(!updateOffer?.token || !updateOffer.available)return;
    updateBusy=true;checkButton.disabled=true;applyButton.disabled=true;
    updateStatus.textContent='Preparing the verified update. Orca may reload this panel…';
    try{
      const result=await window.orca.plugin.request('updates.apply',{token:updateOffer.token},{timeoutMs:300000});
      if(result.state==='approval_required'){
        updateStatus.textContent='Orca requires approval before applying this package. Review its exact command below.';
        approval.textContent=result.message;approval.hidden=false;document.querySelector('#updates').open=true;
      }else{updateStatus.textContent=result.message;applyButton.hidden=true;updateOffer=null;}
    }catch(error){updateStatus.textContent=`Update not applied: ${error.message}`;}
    finally{updateBusy=false;checkButton.disabled=false;applyButton.disabled=false;}
  }
  function reset(){revision++;context=null;window.jtasksTransport.ready=false;api?.reset();document.querySelector('#run-label').hidden=true;document.querySelector('#workspace').textContent='Current project';document.querySelector('#cli').textContent='';}
  function message(text){const box=document.createElement('div');box.className='loading-state';box.textContent=text;main.replaceChildren(box);}
  async function connect(selected) {
    reset();const ticket=revision;
    message('Preparing native CLI and reading your project…');
    try {
      const data=await window.orca.plugin.request('context',selected?{run:selected}:{},{timeoutMs:60000});
      if(ticket!==revision)return;
      context=data;document.querySelector('#workspace').textContent=data.workspace || 'Current project';
      run.replaceChildren();
      if(data.runs?.length){
        const placeholder=document.createElement('option');placeholder.textContent='Select a run…';placeholder.value='';placeholder.disabled=true;run.append(placeholder);
        for(const item of data.runs){const option=document.createElement('option');option.value=item.run;option.textContent=item.run;run.append(option);}
        run.value=data.run || '';document.querySelector('#run-label').hidden=false;
      }
      if(data.cli)document.querySelector('#cli').textContent=`Native CLI ${data.cli.version} · ${data.cli.source}\n${data.cli.executable}`;
      if(data.state!=='ready'){
        message(data.message || 'Project state is unavailable.');document.querySelector('#connection').textContent='No task snapshot loaded';return;
      }
      // Task and output links always belong to the newly selected context.
      history.replaceState(null,'','#overview');window.jtasksTransport.ready=true;await api.refresh();
    }catch(e){if(ticket===revision){message(`Unable to read project: ${e.message}`);document.querySelector('#connection').textContent='Use Refresh to retry';}}
  }
  window.jtasksTransport={
    ready:false,
    async read(view,fields){
      const ticket=revision, key=context?.key;
      if(!key)throw new Error('Select a local project run');
      let data;
      try{data=await window.orca.plugin.request('read',{key,view,fields},{timeoutMs:12000});}
      catch(e){if(e.code==='conflict' && ticket===revision)connect();throw e;}
      if(ticket!==revision)throw new Error('Project changed');return data;
    },
    attach(renderer){
      api=renderer;
      document.querySelector('#refresh').onclick=()=>connect(context?.run);
      run.onchange=()=>connect(run.value);
      if(!window.orca?.plugin){message('Open this panel inside Orca. The plugin bridge is unavailable.');return;}
      checkButton.onclick=()=>checkUpdates();applyButton.onclick=applyUpdate;
      checkUpdates(true);
      window.orca.plugin.on('context.changed',()=>connect());
      connect();
    }
  };
})();
