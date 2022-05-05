use std::{collections::{HashMap, BTreeMap}, sync::Arc, cmp::Ordering, io::Write, mem};

use anyhow::Result;
use memfile::{MemFile, CreateOptions, Seal};
use reqwest::Method;
use serde::Deserialize;
use tokio::sync::{Mutex, oneshot, MutexGuard};

use crate::{choice::Choices, app::{journals::AppJournalList, marking::AppMarking}};

#[derive(Default)]
pub struct Globals {
    cgi_endpoint:  String,
    pager_command: String,
    choices:       Choices,
}

impl Globals {
    pub fn new(cgi_endpoint: String, pager_command: String, choices: Choices) -> Self {
        Self {
            cgi_endpoint,
            pager_command,
            choices,
        }
    }

    pub fn cgi_endpoint(&self) -> &str {
        &self.cgi_endpoint
    }

    pub fn pager_command(&self) -> &str {
        &self.pager_command
    }

    pub fn choices(&self) -> &Choices {
        &self.choices
    }
}

#[derive(Debug, Clone, Default)]
pub struct Authentication {
    username: String,
    password: String,
}

impl Authentication {
    pub fn new(username: String, password: String) -> Self {
        Self {
            username,
            password,
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &str {
        &self.password
    }
}

#[derive(Default)]
pub struct Journals {
    database: HashMap<JournalTag, Arc<Mutex<Journal>>>,
    ordering: Vec<JournalTag>,
    queue: Vec<JournalLoadReceiver>,
}

pub struct JournalsIter<'a> {
    journals: &'a Journals,
    index: usize,
}

impl<'a> Iterator for JournalsIter<'a> {
    type Item = (&'a JournalTag, Arc<Mutex<Journal>>);

    fn next(&mut self) -> Option<Self::Item> {
        let tag = self.journals.ordering.get(self.index)?;
        let journal = self.journals.database.get(tag)
            .expect("ordering is out of sync with database");

        self.index += 1;

        Some((tag, journal.clone()))
    }
}

impl Journals {
    pub fn new() -> Self {
        Self {
            database: HashMap::new(),
            ordering: Vec::new(),
            queue: Vec::new(),
        }
    }

    pub fn iter(&self) -> JournalsIter<'_> {
        JournalsIter {
            journals: self,
            index: 0,
        }
    }

    pub fn insert(&mut self, tag: JournalTag, journal_meta: JournalMeta) {
        self.database.insert(tag.clone(), Arc::new(Mutex::new(Journal::Unloaded(journal_meta))));
        match self.ordering.binary_search(&tag) {
            Ok(index) | Err(index) => {
                self.ordering.insert(index, tag);
            }
        }
    }

    pub async fn get<'s>(&'s self, tag: &JournalTag) -> Option<MutexGuard<'s, Journal>> {
        match self.database.get(tag) {
            Some(journal) => Some(journal.lock().await),
            None => None,
        }
    }

    pub fn len(&self) -> usize {
        let len = self.database.len();
        assert_eq!(len, self.ordering.len());
        
        len
    }

    pub fn queue_load(&mut self, tag: &JournalTag, cgi_endpoint: &str, auth: &Authentication) -> Result<()> {
        let journal = self.database.get(tag)
            .ok_or_else(|| anyhow::anyhow!("Tried to load non-existent journal: {tag:?}"))?;

        let (sender, receiver) = oneshot::channel();

        self.queue.push(receiver);

        tokio::spawn(
            load_journal(
                tag.clone(),
                journal.clone(),
                cgi_endpoint.to_string(),
                auth.clone(),
                sender,
            )
        );

        Ok(())
    }

    pub fn scan_queue(&mut self) -> Result<()> {
        let mut happy_to_drop = vec![];

        for (index, receiver) in self.queue.iter_mut().enumerate() {
            if let Ok(res) = receiver.try_recv() {
                res?;
                happy_to_drop.push(index);
            }
        }

        // ensure we drop from the end to the beginning
        happy_to_drop.sort_unstable_by(|a, b| b.cmp(a));

        for index in happy_to_drop {
            self.queue.remove(index);
        }

        Ok(())
    }
}

pub struct JournalLoadApp {
    cgi_endpoint: String,
    auth: Authentication,
}

impl<B> From<&AppJournalList<B>> for JournalLoadApp {
    fn from(app: &AppJournalList<B>) -> Self {
        Self {
            cgi_endpoint: app.globals().cgi_endpoint().to_string(),
            auth: app.auth().clone(),
        }
    }
}

impl<B> From<&AppMarking<B>> for JournalLoadApp {
    fn from(app: &AppMarking<B>) -> Self {
        Self {
            cgi_endpoint: app.globals().cgi_endpoint().to_string(),
            auth: app.auth().clone(),
        }
    }
}

async fn load_journal(
    tag:     JournalTag,
    journal: Arc<Mutex<Journal>>,
    cgi_endpoint: String,
    auth: Authentication,
    sender:  JournalLoadSender,
) {
    let mut journal = journal.lock().await;
    if journal.is_loaded() {
        return;
    }

    let body = || async move {
        #[derive(Deserialize)]
        pub struct SubmissionJson {
            files: BTreeMap<String, FileJson>,
            marks: BTreeMap<String, MarkJson>,
        }
        
        #[derive(Deserialize)]
        pub struct FileJson {
            name: String,
            contents: String,
        }
        
        #[derive(Deserialize)]
        pub struct MarkJson {
            name: String,
            text: String,
        }

        let assignment = tag.assignment();
        let group_id   = tag.group_id();
        let student_id = tag.student_id();

        let full_endpoint = format!("{cgi_endpoint}/api/v1/assignments/{assignment}/submissions/{group_id}/{student_id}/");

        let client = reqwest::Client::new();
        let resp: SubmissionJson = client.request(Method::GET, full_endpoint)
            .basic_auth(auth.username(), Some(auth.password()))
            .send()
            .await?
            .json()
            .await?;

        let mut submission_files = vec![];
        let mut marking_files    = vec![];

        for (_index, file) in resp.files {
            let mut mem_file = MemFile::create("memfile", CreateOptions::new().allow_sealing(true))?;
            mem_file.write_all(file.contents.as_bytes())?;
            mem_file.add_seals(Seal::Write | Seal::Shrink | Seal::Grow)?;

            submission_files.push(JournalFile::new(file.name, mem_file));
        }

        for (_index, file) in resp.marks {
            let mut mem_file = MemFile::create("memfile", CreateOptions::new().allow_sealing(true))?;
            mem_file.write_all(file.text.as_bytes())?;
            mem_file.add_seals(Seal::Write | Seal::Shrink | Seal::Grow)?;

            marking_files.push(JournalFile::new(file.name, mem_file));
        }

        let journal_data = JournalData::new(submission_files, marking_files);

        match &mut *journal {
            Journal::Unloaded(meta) => {
                *journal = Journal::Loaded(mem::take(meta), journal_data);
            }
            _ => unreachable!("checked at the beginning of body, and we hold lock"),
        }

        anyhow::Ok(())
    };

    sender.send(body().await)
        .expect("receiver should not drop before sender");
}

pub type JournalLoadReceiver = oneshot::Receiver<Result<()>>;
pub type JournalLoadSender   = oneshot::Sender  <Result<()>>;

pub enum Journal {
    Unloaded(JournalMeta),
    Loaded(JournalMeta, JournalData),
}

impl Journal {
    pub fn meta(&self) -> &JournalMeta {
        match self {
            Self::Unloaded(meta)    => meta,
            Self::Loaded  (meta, _) => meta,
        }
    }

    pub fn data(&self) -> Option<&JournalData> {
        match self {
            Self::Unloaded(_)       => None,
            Self::Loaded  (_, data) => Some(data),
        }
    }

    pub fn is_loaded(&self) -> bool {
        match self {
            Self::Unloaded(_)    => false,
            Self::Loaded  (_, _) => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JournalTag {
    assignment: String,
    group_id:   String,
    student_id: String,
}

impl JournalTag {
    pub fn new(assignment: String, group_id: String, student_id: String) -> Self {
        Self {
            assignment,
            group_id,
            student_id,
        }
    }

    pub fn assignment(&self) -> &str {
        &self.assignment
    }

    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    pub fn student_id(&self) -> &str {
        &self.student_id
    }
}

#[derive(Debug, Default)]
pub struct JournalMeta {
    name: String,
    provisional_mark: Option<f64>,
    mark: Option<f64>,
}

impl JournalMeta {
    pub fn new(name: String, provisional_mark: Option<f64>, mark: Option<f64>) -> Self {
        Self {
            name,
            provisional_mark,
            mark,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn provisional_mark(&self) -> Option<f64> {
        self.provisional_mark
    }

    pub fn mark(&self) -> Option<f64> {
        self.mark
    }
}

pub struct JournalData {
    submission_files: Vec<JournalFile>,
    marking_files:    Vec<JournalFile>,
}

impl JournalData {
    pub fn new(submission_files: Vec<JournalFile>, marking_files: Vec<JournalFile>) -> Self {
        Self {
            submission_files,
            marking_files,
        }
    }

    pub fn submission_files(&self) -> &[JournalFile] {
        &self.submission_files
    }

    pub fn marking_files(&self) -> &[JournalFile] {
        &self.marking_files
    }
}

pub struct JournalFile {
    file_name: String,
    file_data: MemFile,
}

impl JournalFile {
    pub fn new(file_name: String, file_data: MemFile) -> Self {
        Self {
            file_name,
            file_data,
        }
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn file_data(&self) -> &MemFile {
        &self.file_data
    }
}

impl PartialEq for JournalMeta {
    fn eq(&self, other: &Self) -> bool {
        if let Some(provisional_mark) = &self.provisional_mark {
            if provisional_mark.is_nan() {
                panic!("provisional_mark is NaN for {self:?}");
            }
        }
        
        if let Some(mark) = &self.mark {
            if mark.is_nan() {
                panic!("mark is NaN for {self:?}");
            }
        }

        self.name == other.name && self.provisional_mark == other.provisional_mark && self.mark == other.mark
    }
}

impl PartialOrd for JournalMeta {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let ordering = match (self.mark, other.mark) {
            (Some(a), Some(b)) => b.partial_cmp(&a).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => {
                match (self.provisional_mark, other.provisional_mark) {
                    (Some(a), Some(b)) => b.partial_cmp(&a).unwrap_or(Ordering::Equal),
                    (Some(_), None) => Ordering::Greater,
                    (None, Some(_)) => Ordering::Less,
                    (None, None) => self.name.cmp(&other.name),
                }
            }
        };

        Some(ordering)
    }
}

impl Eq  for JournalMeta {}
impl Ord for JournalMeta {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).expect("partial_cmp is infallible")
    }
}
