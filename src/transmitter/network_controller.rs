use std::rc::Rc;
use crate::transmitter::transmission_handler::TransmissionHandler;

pub struct NetworkController<'a> {
    transmission_handler: Rc<TransmissionHandler<'a>>,

}

impl<'a> NetworkController<'a> {
    fn new() -> Self {
        todo!()
    }

    fn start_flooding(&self) {
        todo!()
    }
}