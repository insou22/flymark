use std::{collections::{HashMap, BTreeMap}, sync::Arc, cmp::Ordering, io::{Write, Read, Seek}, mem};

use anyhow::Result;
use async_trait::async_trait;
use memfile::{MemFile, CreateOptions, Seal};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, oneshot, MutexGuard};

use crate::{choice::{Choices, ChoiceSelections, Choice}, app::{journals::AppJournalList, marking::AppMarking}, util::task::{TaskRunner, Task}};

#[derive(Clone, Default)]
pub struct Globals {
    inner: Arc<GlobalsInner>,
}

#[derive(Default)]
struct GlobalsInner {
    cgi_endpoint:  String,
    pager_command: String,
    mark_name:     String,
    choices:       Choices,
    preload:       usize,
    panic_on_drop: bool,
    hide_names:    bool,
}

impl Globals {
    pub fn new(cgi_endpoint: String, pager_command: String, mark_name: String, choices: Choices, preload: usize, panic_on_drop: bool, hide_names: bool) -> Self {
        Self {
            inner: Arc::new(GlobalsInner {
                cgi_endpoint,
                pager_command,
                mark_name,
                choices,
                preload,
                panic_on_drop,
                hide_names,
            }),
        }
    }

    pub fn cgi_endpoint(&self) -> &str {
        &self.inner.cgi_endpoint
    }

    pub fn pager_command(&self) -> &str {
        &self.inner.pager_command
    }

    pub fn mark_name(&self) -> &str {
        &self.inner.mark_name
    }

    pub fn choices(&self) -> &Choices {
        &self.inner.choices
    }

    pub fn preload(&self) -> usize {
        self.inner.preload
    }

    pub fn panic_on_drop(&self) -> bool {
        self.inner.panic_on_drop
    }

    pub fn hide_names(&self) -> bool {
        self.inner.hide_names
    }
}

#[derive(Debug, Clone)]
pub struct Authentication {
    inner: Arc<AuthenticationInner>,
}

#[derive(Debug)]
struct AuthenticationInner {
    username: String,
    password: String,
}

impl Authentication {
    pub fn new(username: String, password: String) -> Self {
        Self {
            inner: Arc::new(AuthenticationInner {
                username,
                password,
            }),
        }
    }

    pub fn username(&self) -> &str {
        &self.inner.username
    }

    pub fn password(&self) -> &str {
        &self.inner.password
    }
}

#[derive(Default)]
pub struct Journals {
    database: HashMap<JournalTag, Arc<Mutex<Journal>>>,
    ordering: Vec<(JournalTag, JournalMeta)>,
    queue: Vec<Task<()>>,
    globals: Globals,
}

pub struct JournalsIter<'a> {
    journals: &'a Journals,
    index: usize,
}

pub trait BidirectionalIterator: Iterator {
    fn next_back(&mut self) -> Option<Self::Item>;
}

impl<'a> Iterator for JournalsIter<'a> {
    type Item = (&'a JournalTag, Arc<Mutex<Journal>>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.journals.database.len() {
            return None;
        }

        let (tag, _) = self.journals.ordering.get(self.index)?;
        let journal = self.journals.database.get(tag)
            .expect("ordering is out of sync with database");

        self.index += 1;

        Some((tag, journal.clone()))
    }
}

impl<'a> BidirectionalIterator for JournalsIter<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.index == 0 {
            return None;
        }

        self.index -= 1;

        let (tag, _) = self.journals.ordering.get(self.index)?;
        let journal = self.journals.database.get(tag)
            .expect("ordering is out of sync with database");

        Some((tag, journal.clone()))
    }
}

impl Journals {
    pub fn new(globals: Globals) -> Self {
        Self {
            database: HashMap::new(),
            ordering: Vec::new(),
            queue: Vec::new(),
            globals,
        }
    }

    pub fn iter(&self) -> impl BidirectionalIterator<Item = (&'_ JournalTag, Arc<Mutex<Journal>>)> {
        JournalsIter {
            journals: self,
            index: 0,
        }
    }

    pub fn insert(&mut self, tag: JournalTag, meta: JournalMeta) {
        self.database.insert(tag.clone(), Arc::new(Mutex::new(Journal::Unloaded(UnloadedJournal::new(meta.clone())))));
        
        match self.ordering.binary_search_by_key(&&meta, |(_, meta)| &meta) {
            Ok(index) | Err(index) => {
                self.ordering.insert(index, (tag, meta));
            }
        }
    }

    pub fn try_get<'s>(&'s self, tag: &JournalTag) -> Option<MutexGuard<'s, Journal>> {
        match self.database.get(tag) {
            Some(journal) => journal.try_lock().ok(),
            None => None,
        }
    }

    pub async fn get<'s>(&'s self, tag: &JournalTag) -> Option<MutexGuard<'s, Journal>> {
        match self.database.get(tag) {
            Some(journal) => Some(journal.lock().await),
            None => None,
        }
    }

    pub fn len(&self) -> usize {
        self.database.len()
    }

    pub async fn unload(&mut self, tag: &JournalTag) {
        if let Some(journal) = self.database.get(tag) {
            let mut journal = journal.lock().await;
            *journal = Journal::Unloaded(UnloadedJournal::new(journal.meta().clone()));
        }
    }

    pub fn queue_load(
        &mut self,
        tag: JournalTag,
        cgi_endpoint: &str,
        auth: Authentication,
        mark_name: &str,
    ) -> Result<()> {
        let journal = self.database.get(&tag)
            .ok_or_else(|| anyhow::anyhow!("Tried to load non-existent journal: {tag:?}"))?;

        let task = Task::new(
            LoadJournalTask {
                tag:          tag,
                journal:      journal.clone(),
                cgi_endpoint: cgi_endpoint.to_string(),
                auth:         auth,
                mark_name:    mark_name.to_string(),
            },
            self.globals.panic_on_drop(),
        );

        self.queue.push(task);

        Ok(())
    }

    pub fn queue_mark(
        &mut self,
        tag:     JournalTag,
        choices: ChoiceSelections,
        cgi_endpoint: &str,
        auth: Authentication,
        mark_name: &str,
    ) -> Result<()> {
        let journal = self.database.get(&tag)
            .ok_or_else(|| anyhow::anyhow!("Tried to load non-existent journal: {tag:?}"))?;

        let task = Task::new(
            MarkJournalTask {
                choices:      choices,
                journal_tag:  tag,
                journal:      journal.clone(),
                cgi_endpoint: cgi_endpoint.to_string(),
                auth:         auth,
                mark_name:    mark_name.to_string(),
            },
            self.globals.panic_on_drop(),
        );

        self.queue.push(task);

        Ok(())
    }

    pub fn scan_queue(&mut self) -> Result<usize> {
        let mut happy_to_drop = vec![];

        for (index, task) in self.queue.iter_mut().enumerate() {
            if let Some(_) = task.poll()? {
                happy_to_drop.push(index);
            }
        }

        // ensure we drop from the end to the beginning
        happy_to_drop.sort_unstable_by(|a, b| b.cmp(a));

        for index in happy_to_drop {
            self.queue.remove(index);
        }

        Ok(self.queue.len())
    }

    pub fn queue_size(&self) -> usize {
        self.queue.len()
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

struct LoadJournalTask {
    tag: JournalTag,
    journal: Arc<Mutex<Journal>>,
    cgi_endpoint: String,
    auth: Authentication,
    mark_name: String,
}

#[async_trait]
impl TaskRunner<()> for LoadJournalTask {
    async fn run(self) -> Result<()> {
        let mut journal = self.journal.lock().await;
        if journal.is_loaded() {
            return Ok(());
        }
        drop(journal);

        #[derive(Deserialize)]
        pub struct SubmissionJson {
            files: BTreeMap<String, FileJson>,
            marks: BTreeMap<String, MarkJson>,
            metadata: MetadataJson,
        }
        
        #[derive(Deserialize)]
        pub struct FileJson {
            name: String,
            contents: String,
        }
        
        #[derive(Deserialize)]
        pub struct MarkJson {
            name: String,
            text: Option<String>,
        }

        #[derive(Deserialize)]
        pub struct MetadataJson {
            mark: Option<f64>,
        }

        let assignment   = self.tag.assignment();
        let group_id     = self.tag.group_id();
        let student_id   = self.tag.student_id();
        let cgi_endpoint = self.cgi_endpoint;

        let full_endpoint = format!("{cgi_endpoint}/api/v1/assignments/{assignment}/submissions/{group_id}/{student_id}/");

        let client = reqwest::Client::new();
        let resp: SubmissionJson = client.request(Method::GET, full_endpoint)
            .basic_auth(self.auth.username(), Some(self.auth.password()))
            .send()
            .await?
            .json()
            .await?;

        let mut submission_files = vec![];
        let mut marking_files    = vec![];

        for (imark_id, file) in resp.files {
            let mut mem_file = MemFile::create("memfile", CreateOptions::new().allow_sealing(true))?;
            mem_file.write_all(file.contents.as_bytes())?;
            mem_file.add_seals(Seal::Write | Seal::Shrink | Seal::Grow)?;

            let imark_id_usize = str::parse::<usize>(&imark_id)?;

            submission_files.push(JournalFile::new(imark_id_usize, file.name, mem_file));
        }

        for (imark_id, file) in resp.marks {
            let mut mem_file = MemFile::create("memfile", CreateOptions::new().allow_sealing(true))?;
            if let Some(text) = file.text {
                mem_file.write_all(text.as_bytes())?;
            }
            mem_file.add_seals(Seal::Write | Seal::Shrink | Seal::Grow)?;

            let imark_id_usize = str::parse::<usize>(&imark_id)?;

            marking_files.push(JournalFile::new(imark_id_usize, file.name, mem_file));
        }

        if !marking_files.iter().any(|file| file.file_name() == &self.mark_name) {
            let mut mem_file = MemFile::create("memfile", CreateOptions::new().allow_sealing(true))?;
            mem_file.add_seals(Seal::Write | Seal::Shrink | Seal::Grow)?;

            let imark_id_usize = marking_files.iter()
                .map(JournalFile::imark_id)
                .max()
                .unwrap_or(0) + 1;

            marking_files.push(JournalFile::new(imark_id_usize, self.mark_name.to_string(), mem_file));
        }

        let journal_data = JournalData::new(submission_files, marking_files);

        let mut journal = self.journal.lock().await;
        if journal.is_loaded() {
            return Ok(());
        }
        journal.meta_mut().mark = resp.metadata.mark;

        *journal = Journal::Loaded(LoadedJournal::new(mem::take(journal.meta_mut()), journal_data));

        Ok(())
    }
}

struct MarkJournalTask {
    choices:      ChoiceSelections,
    journal_tag:  JournalTag,
    journal:      Arc<Mutex<Journal>>,
    cgi_endpoint: String,
    auth:         Authentication,
    mark_name:    String,
}

#[async_trait]
impl TaskRunner<()> for MarkJournalTask {
    async fn run(self) -> Result<()> {
        let mut mark = 0.0;
        let mut comments = vec![];
    
        for choice in self.choices.selections().iter()
            .filter(|selection| selection.selected())
            .map(|selection| selection.choice())
        {
            match choice {
                Choice::Plus(n, comment) => {
                    mark += *n;
                    comments.push(format!("+{n} {comment}"));
                }
                Choice::Minus(n, comment) => {
                    mark -= *n;
                    comments.push(format!("-{n} {comment}"));
    
                }
                Choice::Set(n, comment) => {
                    mark = *n;
                    comments.push(format!("{n} {comment}"));
                }
                Choice::Comment(_)  => unreachable!(),
            }
        }

        if mark < 0.0 {
            mark = 0.0;
        }

        let mark = (mark * 100.0).round() / 100.0;
    
        let (imark_id, journal_mark_name, mut journal_mark_text) = {
            let mut lock = self.journal.lock().await;

            let mut data = lock.data_mut().expect("journal must be loaded to mark");

            let mut marking_file = data.marking_files.iter_mut()
                .find(|file| file.file_name() == &self.mark_name)
                .unwrap_or_else(|| panic!("{} mark always exists", self.mark_name));

            let mut text = String::new();
            marking_file.file_data.seek(std::io::SeekFrom::Start(0))?;
            marking_file.file_data.read_to_string(&mut text)?;

            (marking_file.imark_id(), marking_file.file_name().to_string(), text)
        };

        #[derive(Serialize)]
        struct MarkPut {
            marks: BTreeMap<String, Mark>,
            comments: BTreeMap<String, ()>,
        }

        #[derive(Serialize)]
        struct Mark {
            at: String,
            by: String,
            name: String,
            is_final: bool,
            mark: f64,
            text: String,
        }
    
        let mut body = MarkPut {
            marks: BTreeMap::new(),
            comments: BTreeMap::new(),
        };
    
        let at = chrono::Local::now().format("%F %T%.6f").to_string();
        let by = self.auth.username().to_string();
    
        journal_mark_text += &format!("\nmarked with flymark by {by} at {at}\n\n");

        for comment in comments {
            journal_mark_text += &comment;
            journal_mark_text += "\n";
        }
    
        body.marks.insert(
            format!("{imark_id}"),
            Mark {
                at,
                by,
                is_final: true,
                mark,
                name: journal_mark_name,
                text: journal_mark_text,
            }
        );
    
        let imark  = self.cgi_endpoint;
        let assign = &self.journal_tag.assignment;
        let group  = &self.journal_tag.group_id;
        let stuid  = &self.journal_tag.student_id;
        
        let endpoint = format!("{imark}/api/v1/assignments/{assign}/submissions/{group}/{stuid}/");

        reqwest::Client::new()
            .put(endpoint)
            .basic_auth(self.auth.username(), Some(self.auth.password()))
            .json(&body)
            .send()
            .await?
            .text()
            .await?;
        
        Ok(())
    }
}

#[derive(Debug)]
pub enum Journal {
    Unloaded(UnloadedJournal),
    Loaded  (LoadedJournal),
}

impl Journal {
    pub fn meta(&self) -> &JournalMeta {
        match self {
            Self::Unloaded(UnloadedJournal { meta })
            | Self::Loaded(LoadedJournal { meta, .. }) => meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut JournalMeta {
        match self {
            Self::Unloaded(UnloadedJournal { meta })
            | Self::Loaded(LoadedJournal { meta, .. }) => meta,
        }
    }

    pub fn data(&self) -> Option<&JournalData> {
        match self {
            Self::Unloaded(_) => None,
            Self::Loaded(LoadedJournal { meta: _, data }) => Some(data),
        }
    }

    pub fn data_mut(&mut self) -> Option<&mut JournalData> {
        match self {
            Self::Unloaded(_) => None,
            Self::Loaded(LoadedJournal { meta: _, data }) => Some(data),
        }
    }

    pub fn is_loaded(&self) -> bool {
        match self {
            Self::Unloaded(_) => false,
            Self::Loaded  (_) => true,
        }
    }
}

#[derive(Debug)]
pub struct UnloadedJournal {
    meta: JournalMeta,
}

impl UnloadedJournal {
    pub fn new(meta: JournalMeta) -> Self {
        Self { meta }
    }

    pub fn meta(&self) -> &JournalMeta {
        &self.meta
    }
}

#[derive(Debug)]
pub struct LoadedJournal {
    meta: JournalMeta,
    data: JournalData,
}

impl LoadedJournal {
    pub fn new(meta: JournalMeta, data: JournalData) -> Self {
        Self { meta, data }
    }

    pub fn meta(&self) -> &JournalMeta {
        &self.meta
    }

    pub fn data(&self) -> &JournalData {
        &self.data
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
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

#[derive(Debug, Default, Clone)]
pub struct JournalMeta {
    name: String,
    provisional_mark: Option<f64>,
    mark: Option<f64>,
    notes: Option<String>,
}

impl JournalMeta {
    pub fn new(name: String, provisional_mark: Option<f64>, mark: Option<f64>, notes: Option<String>) -> Self {
        Self {
            name,
            provisional_mark,
            mark,
            notes,
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

    pub fn notes(&self) -> Option<&str> {
        self.notes.as_deref()
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
pub struct JournalFile {
    imark_id: usize,
    file_name: String,
    file_data: MemFile,
}

impl JournalFile {
    pub fn new(imark_id: usize, file_name: String, file_data: MemFile) -> Self {
        Self {
            imark_id,
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

    pub fn imark_id(&self) -> usize {
        self.imark_id
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
        let mark_ordering = match (self.mark, other.mark) {
            (Some(a), Some(b)) => b.partial_cmp(&a).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None)    => Ordering::Equal,
        };

        let provisional_mark_ordering = match (self.provisional_mark, other.provisional_mark) {
            (Some(a), Some(b)) => b.partial_cmp(&a).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None)    => Ordering::Equal,
        };

        let name_ordering = self.name.cmp(&other.name);

        Some(mark_ordering.then(provisional_mark_ordering).then(name_ordering))
    }
}

impl Eq  for JournalMeta {}
impl Ord for JournalMeta {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).expect("partial_cmp is infallible")
    }
}
