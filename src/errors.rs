use youtrack_rs;

error_chain! {

    links {
        YouTrack(youtrack_rs::errors::Error, youtrack_rs::errors::ErrorKind);
    }

    foreign_links {
        Telegram(::telegram_bot::Error);
        Io(::std::io::Error);
    }

}
