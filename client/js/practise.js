export async function getSources() {
  const response = await fetch("assets/assets.json");
  const list = await response.json();
  let sources = [];
  for (const item of list) {
    let url = "assets/" + item + "/metadata.json";
    let response = await fetch(url);
    let metadata = await response.json();
    await sources.push({ directory: item, metadata: metadata });
  }
  return sources;
}

export async function populateSourceSelector() {
  const sources = await getSources();
  const source_selector = document.getElementById("source-selector");
  for (var i = 0; i < sources.length; i++) {
    const item = sources[i];
    console.log("item=" + JSON.stringify(item));
    let option = document.createElement("option");
    option.text = item.metadata.name;
    option.value = item.directory;
    source_selector.add(option);
  }
}

export async function sourceChanged() {
  let source_selector = document.getElementById("source-selector");
  let source_dir = source_selector.value;
  if (source_dir === "") {
    return;
  }
  let asset_dir = "assets/" + source_dir + "/";
  let response = await fetch(asset_dir + "metadata.json");
  let metadata = await response.json();
  console.log(metadata);
  let player = document.getElementById("player");
  player.src = asset_dir + metadata.audio;
  let player_figure = document.getElementById("player-figure");
  let player_figure_caption = document.getElementById("player-caption");
  player_figure_caption.innerHTML = metadata.name;
}

