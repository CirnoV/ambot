use std::collections::HashMap;
use std::fs::OpenOptions;

use serenity::framework::standard::{macros::command, Args, CommandResult, CommandError};
use serenity::model::prelude::*;
use serenity::prelude::*;
use serenity::builder::CreateEmbed;

use gdcrunner::gameinfo::*;
use gdcrunner::downloader::GDCError;
use gdcrunner::{GDCManager};

use crate::utls::discordhelpers;
use crate::cache::{BotInfo, Sqlite};
use crate::utls::constants::{ICON_GDC, COLOR_GDC};
use std::process::{Command, Stdio};
use rusqlite::params;
use gdcrunner::gdcrunner::GameData;
use std::sync::Arc;
use serenity::CacheAndHttp;

use crate::steam::pics_bindings::*;
use serenity::http::Http;
use crate::utls::discordhelpers::manual_dispatch;

#[command]
#[owners_only]
#[sub_commands(add, list, remove)]
pub async fn gdc(ctx: &Context, msg: &Message, args: Args) -> CommandResult {
    if args.is_empty() {
        return Err(CommandError::from(
            "Please supply a game to execute gamedata checker on.\n\nExample: -gdc csgo",
        ));
    }
    let app = args.parse::<String>().unwrap();

    let cache = GameCache::new();
    let game_option = cache.lookup_shortname(&app);
    if game_option.is_none() {
        return Err(CommandError::from(
            "Invalid or unsupported target.",
        ));
    }
    let game = game_option.unwrap().clone();
    let mut log = Vec::new();

    let mut emb = CreateEmbed::default();
    emb.title("Gamedata Checker");
    emb.thumbnail(ICON_GDC);
    emb.color(COLOR_GDC);
    log.push(String::from("Pulling latest sourcemod..."));
    emb.description(format!("```\n{}\n```", log.join("\n")));
    emb.footer(|f| f.text(format!("Requested by: {}", &msg.author.tag())));

    let mut emb_msg = discordhelpers::embed_message(emb);
    let message = msg.channel_id
        .send_message(&ctx.http, |_| &mut emb_msg)
        .await?;

    run_gdc(message, Some(&msg.author.tag()), ctx.data.clone(), ctx.http.clone(), log, game).await
}

async fn build_results_embed(tag : Option<&String>, msg : &mut Message, http : &Arc<Http>, results : &HashMap<String, std::result::Result<bool, GDCError>>, log : & mut Vec<String>, game : &Game) {
    msg.edit(http, |f| {
        f.embed(|e| {
            for (k, v) in results {
                if v.is_err() {
                    e.field(k, format!("```\n{}\n```", v.as_ref().err().unwrap()), false);
                }
            }

            e.color(COLOR_GDC);
            log.push(String::from("Execution completed."));
            e.description(format!("```\n{}\n```", log.join("\n")));
            e.thumbnail(ICON_GDC);
            if let Some(tag) = tag {
                e.title("Gamedata Checker");
                e.footer(|f| f.text(format!("Requested by: {}", tag)));
            }
            else {
                e.title(format!("{} update detected", game.name));
            }
            e
        })
    }).await.expect("Unable to edit message.")
}

async fn update_msg(tag : Option<&String>, msg : &mut Message, http : &Arc<Http>, text : &str, log : & mut Vec<String>, game : &Game) {

    msg.edit(http, |f| f.embed(|e| {
        log.push(text.to_owned());
        e.description(format!("```\n{}\n```", log.join("\n")));
        e.thumbnail(ICON_GDC);
        e.color(COLOR_GDC);
        if let Some(tag) = tag {
            e.title("Gamedata Checker");
            e.footer(|f| f.text(format!("Requested by: {}", tag)));
        }
        else {
            e.title(format!("{} update detected", game.name));
        }
        e
    })).await.expect("Unable to edit message.")
}

fn get_appid_from_str(appid_str : String) -> Result<i32, CommandError> {
    let appid;
    if let Ok(appid_int) = appid_str.parse::<i32>() {
        appid = appid_int;
    }
    else {
        let man = GameCache::new();
        if let Some(g) = man.lookup_shortname(&appid_str) {
            appid = g.appid
        } else {
            return Err(CommandError::from(format!("Unable to resolve application \"{}\"", appid_str)));
        }
    }
    return Ok(appid);
}

#[command]
pub async fn add(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    if args.len() != 3 {
        msg.reply(&ctx, "Invalid syntax: -gdc add <appid> <url OR path> <source>").await?;
        return Ok(())
    }

    let appid_str = args.single::<String>()?;
    let appid = get_appid_from_str(appid_str)?;

    let src_type = args.single::<String>()?;
    let src = args.single::<String>()?;

    let data = ctx.data.read().await;
    let conn = data.get::<Sqlite>().unwrap().lock().await;

    if src_type == "url" {
        conn.execute("INSERT INTO gamedata (appid, url, path) VALUES (?1, ?2, ?3)",
        params![appid, src, ""])?;
    }
    else if src_type == "path" {
        conn.execute("INSERT INTO gamedata (appid, url, path) VALUES (?1, ?2, ?3)",
                     params![appid, "", src])?;
    }
    else {
        msg.reply(&ctx, format!("Invalid source type: `{}`", src)).await?;
        return Ok(())
    }

    msg.reply(&ctx, format!("Added {} to sources list", src)).await?;
    Ok(())
}

#[command]
pub async fn list(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    if args.len() != 1 {
        msg.reply(&ctx, "Invalid syntax: -gdc list <appid>").await?;
        return Ok(())
    }

    let appid_str = args.single::<String>()?;
    let appid = get_appid_from_str(appid_str)?;

    let data = ctx.data.read().await;
    let conn = data.get::<Sqlite>().unwrap().lock().await;

    let list = {
        let mut stmt = conn.prepare("SELECT url, path FROM gamedata WHERE appid = ?")?;
        let person_iter = stmt.query_map(params![appid], |row| {
            Ok(GameData {
                appid,
                url: row.get(0)?,
                path: row.get(1)?,
            })
        })?;

        let mut list = String::from(format!("Sources for {}\n", appid));
        for person in person_iter {
            let p = person.unwrap();
            list.push_str(&format!(" - {}{}\n", p.url, p.path));
        }
        list
    };
    msg.reply(&ctx, format!("```\n{}\n```", list)).await?;
    Ok(())
}

#[command]
pub async fn remove(ctx: &Context, msg: &Message, mut args: Args) -> CommandResult {
    if args.len() != 2 {
        msg.reply(&ctx, "Invalid syntax: -gdc remove <appid> <src>").await?;
        return Ok(())
    }

    let appid_str = args.single::<String>()?;
    let appid = get_appid_from_str(appid_str)?;
    let src = args.single::<String>()?;

    let data = ctx.data.read().await;
    let conn = data.get::<Sqlite>().unwrap().lock().await;

    {
        let mut stmt = conn.prepare("DELETE FROM gamedata WHERE appid = ? AND (url = ? OR path = ?)")?;
        stmt.execute(params![appid, src, src])?;
    };
    msg.reply(&ctx, "I probably removed what you wanted").await?;
    Ok(())
}

async fn run_gdc(mut message : Message, author : Option<&String>, data : Arc<RwLock<TypeMap>>, http: Arc<Http>, mut log : Vec<String>, game : Game) -> CommandResult {
    let data = data.read().await;
    let info = data.get::<BotInfo>().unwrap().read().await;

    let sourcemod_dir = info.get("SOURCEMOD_DIR").unwrap();
    let downloads_dir = info.get("DOWNLOADS_DIR").unwrap();
    let depot_dir = info.get("DEPOT_DIR").unwrap();

    // grab latest sourcemod
    let sourcemod_update = Command::new("git")
        .current_dir(sourcemod_dir)
        .arg("pull")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    match sourcemod_update {
        Ok(mut child) => {
            let wait = child.wait();
            if wait.is_err() {
                update_msg(author, & mut message, &http, &format!("SourceMod pull failed! (git exited with {})", wait.err().unwrap()), & mut log, &game).await
            }
        }
        Err(e) => {
            update_msg(author, & mut message, &http, &format!("SourceMod pull failed: {}", e), & mut log, &game).await
        }
    }

    update_msg(author, & mut message, &http, &format!("Downloading appid '{}'", game.appid), & mut log, &game).await;

    let gdc = GDCManager::new(game.clone(), sourcemod_dir, downloads_dir, depot_dir);
    match gdc.download_game().await {
        Ok(t) => {
            if !t.success() {
                update_msg(author, & mut message, &http, &format!("Exited with status code {}", t), & mut log, &game).await
            }
        }
        Err(e) => {
            update_msg(author, & mut message, &http, &format!("Fatal error: {}", e), & mut log, &game).await
        }
    }

    update_msg(author, & mut message, &http, "Download completed. Running gdc...", & mut log, &game).await;
    let mut file = OpenOptions::new()
        .write(true) // <--------- this
        .create(true)
        .open("output.log")
        .unwrap();


    let list : Vec<GameData> = {
        let conn = data.get::<Sqlite>().unwrap().lock().await;
        let mut stmt = conn.prepare("SELECT url, path FROM gamedata WHERE appid = ?")?;
        let person_iter = stmt.query_map(params![game.appid], |row| {
            Ok(GameData {
                appid: game.appid,
                url: row.get(0)?,
                path: row.get(1)?,
            })
        })?;

        person_iter.map(|p| p.unwrap()).collect()
    };
    let results = gdc.check_gamedata(& mut file, list).await;

    build_results_embed(author, &mut message, &http, &results, & mut log, &game).await;
    message.channel_id.send_files(&http, vec!["output.log"], |f| {
        f
    }).await?;
    std::fs::remove_file("output.log")?;
    Ok(())
}

pub async fn on_update(data: Arc<RwLock<TypeMap>>, http : Arc<CacheAndHttp>, id : u64, app : App) -> () {
    let cache = GameCache::new();
    let mut game = cache.lookup_appid(id as i32).expect("Unknown AppId").clone();

    let mut log = Vec::new();

    let mut emb = CreateEmbed::default();
    emb.title(format!("{} update detected", app.common.name));
    emb.thumbnail(ICON_GDC);
    emb.color(COLOR_GDC);
    log.push(String::from("Pulling latest sourcemod..."));
    emb.description(format!("```\n{}\n```", log.join("\n")));

    let data_lock = data.read().await;
    let bot_info = data_lock.get::<BotInfo>().unwrap().read().await;
    let channel = bot_info.get("PLUGIN_CHANNEL").unwrap();

    let message = manual_dispatch(http.http.clone(), channel.parse::<u64>().unwrap(), emb)
        .await
        .expect("Unable to send msg");

    game.name = app.common.name.clone();
    let _ = run_gdc(message, None, data.clone(), http.http.clone(), log, game).await;

}